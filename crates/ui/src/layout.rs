use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use xml::attribute::OwnedAttribute;
use xml::common::Position;
use xml::common::TextPosition;
use xml::reader::{EventReader, ParserConfig2, XmlEvent};

const MAX_DEPTH: usize = 64;
const MAX_WIDGETS: usize = 16_384;

/// A complete, immutable widget tree instantiated from one XML layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetTree {
    root: Widget,
}

impl WidgetTree {
    /// Parses a UTF-8 layout. `source` names the asset in diagnostics.
    pub fn parse(source: &str, xml: &str) -> Result<Self, LayoutError> {
        Parser {
            source,
            reader: ParserConfig2::new()
                .allow_multiple_root_elements(false)
                .create_reader(xml.as_bytes()),
            remaining: MAX_WIDGETS,
        }
        .parse()
    }

    /// Returns the layout's root panel.
    pub fn root(&self) -> &Widget {
        &self.root
    }
}

/// A widget's authored data and its children in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Widget {
    id: Option<String>,
    classes: Vec<String>,
    kind: WidgetKind,
    binding: Option<String>,
    action: Option<String>,
    children: Arc<[Widget]>,
}

impl Widget {
    /// Returns the authored identifier, unique within its template or layout scope.
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Returns the authored CSS classes in order.
    pub fn classes(&self) -> &[String] {
        &self.classes
    }

    /// Returns the widget type and its authored content.
    pub fn kind(&self) -> &WidgetKind {
        &self.kind
    }

    /// Name of the game-published text for this label or button.
    pub fn binding(&self) -> Option<&str> {
        self.binding.as_deref()
    }

    /// Name of the action declared by this button.
    pub fn action(&self) -> Option<&str> {
        self.action.as_deref()
    }

    /// Returns the panel's children, or an empty slice for a leaf.
    ///
    /// Instances can share immutable child data. A child's position in the tree
    /// determines its instance, independently of its memory address.
    pub fn children(&self) -> &[Widget] {
        &self.children
    }

    fn height(&self) -> usize {
        1 + self.children.iter().map(Self::height).max().unwrap_or(0)
    }
}

/// The supported XML widget types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WidgetKind {
    /// A container for other widgets.
    Panel,
    /// A run of authored text.
    Label {
        /// Text to display.
        text: String,
    },
    /// An image referenced by an asset path.
    Image {
        /// The authored asset reference.
        src: String,
    },
    /// A button with an authored caption.
    Button {
        /// Text to display.
        text: String,
    },
}

/// A syntax or layout error at a one-based source line and column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutError {
    /// Asset name supplied to [`WidgetTree::parse`].
    pub source: String,
    /// One-based line in the source.
    pub line: u64,
    /// One-based column in the source.
    pub column: u64,
    /// Description of the invalid markup.
    pub message: String,
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.source, self.line, self.column, self.message
        )
    }
}

impl std::error::Error for LayoutError {}

struct Parser<'a> {
    source: &'a str,
    reader: EventReader<&'a [u8]>,
    remaining: usize,
}

struct Tag {
    name: String,
    attributes: Vec<OwnedAttribute>,
    position: TextPosition,
}

impl Tag {
    fn take(&mut self, name: &str) -> Option<String> {
        let index = self
            .attributes
            .iter()
            .position(|attr| attr.name.namespace.is_none() && attr.name.local_name == name)?;
        Some(self.attributes.remove(index).value)
    }

    fn classes(&mut self) -> Vec<String> {
        self.take("class")
            .map(|classes| {
                classes
                    .split_ascii_whitespace()
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }
}

struct Template {
    panel: Widget,
    widgets: usize,
    height: usize,
}

impl Parser<'_> {
    fn parse(mut self) -> Result<WidgetTree, LayoutError> {
        let Some(ui) = self.child()? else {
            return Err(self.error(self.reader.position(), "expected <ui>"));
        };
        if ui.name != "ui" {
            return Err(self.error(ui.position, "expected <ui>"));
        }
        self.finish_attributes(&ui)?;

        let mut templates = HashMap::new();
        let mut tag = self.child()?;
        while let Some(mut definition) = tag.take_if(|tag| tag.name == "template") {
            let name = self.required(&mut definition, "name")?;
            self.check_token(&definition, "name", &name)?;
            if templates.contains_key(&name) {
                return Err(self.error(definition.position, format!("duplicate template {name:?}")));
            }
            let remaining = self.remaining;
            let panel = self.panel(&mut definition, None, 1, &mut HashSet::new(), &templates)?;
            templates.insert(
                name,
                Template {
                    height: panel.height(),
                    widgets: remaining - self.remaining,
                    panel,
                },
            );
            tag = self.child()?;
        }

        let root = match tag {
            Some(tag) if tag.name == "panel" => {
                self.widget(tag, 1, &mut HashSet::new(), &templates)?
            }
            _ => return Err(self.error(self.reader.position(), "expected a root <panel>")),
        };
        if let Some(extra) = self.child()? {
            return Err(self.error(extra.position, "expected </ui> after the root panel"));
        }
        if let Some(extra) = self.child()? {
            return Err(self.error(extra.position, "expected the end of the layout"));
        }
        Ok(WidgetTree { root })
    }

    fn widget(
        &mut self,
        mut tag: Tag,
        depth: usize,
        ids: &mut HashSet<String>,
        templates: &HashMap<String, Template>,
    ) -> Result<Widget, LayoutError> {
        let id = tag.take("id");
        if let Some(id) = &id {
            self.check_token(&tag, "id", id)?;
            if !ids.insert(id.clone()) {
                return Err(self.error(tag.position, format!("duplicate widget id {id:?}")));
            }
        }
        if tag.name == "panel" {
            return self.panel(&mut tag, id, depth, ids, templates);
        }

        let classes = tag.classes();
        let binding = if matches!(tag.name.as_str(), "label" | "button") {
            self.optional_token(&mut tag, "bind")?
        } else {
            None
        };
        let action = if tag.name == "button" {
            self.optional_token(&mut tag, "action")?
        } else {
            None
        };
        let widget = match tag.name.as_str() {
            "label" => WidgetKind::Label {
                text: self.required(&mut tag, "text")?,
            },
            "image" => WidgetKind::Image {
                src: self.required(&mut tag, "src")?,
            },
            "button" => WidgetKind::Button {
                text: self.required(&mut tag, "text")?,
            },
            "use" => {
                let name = self.required(&mut tag, "template")?;
                let template = templates.get(&name).ok_or_else(|| {
                    self.error(
                        tag.position,
                        format!("template {name:?} has not been declared"),
                    )
                })?;
                if id.is_none() {
                    return Err(self.error(tag.position, "<use> requires an id"));
                }
                self.reserve(tag.position, depth + template.height - 1, template.widgets)?;
                self.finish_leaf(&tag)?;
                let mut panel = template.panel.clone();
                panel.id = id;
                panel.classes.extend(classes);
                return Ok(panel);
            }
            _ => return Err(self.error(tag.position, format!("unknown widget <{}>", tag.name))),
        };
        self.reserve(tag.position, depth, 1)?;
        self.finish_leaf(&tag)?;
        Ok(Widget {
            id,
            classes,
            kind: widget,
            binding,
            action,
            children: Arc::default(),
        })
    }

    fn panel(
        &mut self,
        tag: &mut Tag,
        id: Option<String>,
        depth: usize,
        ids: &mut HashSet<String>,
        templates: &HashMap<String, Template>,
    ) -> Result<Widget, LayoutError> {
        self.reserve(tag.position, depth, 1)?;
        let classes = tag.classes();
        self.finish_attributes(tag)?;
        let mut children = Vec::new();
        while let Some(child) = self.child()? {
            children.push(self.widget(child, depth + 1, ids, templates)?);
        }
        Ok(Widget {
            id,
            classes,
            kind: WidgetKind::Panel,
            binding: None,
            action: None,
            children: children.into(),
        })
    }

    fn child(&mut self) -> Result<Option<Tag>, LayoutError> {
        loop {
            let event = self
                .reader
                .next()
                .map_err(|error| self.error(error.position(), error.msg()))?;
            let position = self.reader.position();
            if self.reader.doctype().is_some() {
                return Err(self.error(position, "DOCTYPE is unsupported in UI layouts"));
            }
            match event {
                XmlEvent::StartDocument { encoding, .. }
                    if encoding.eq_ignore_ascii_case("UTF-8") => {}
                XmlEvent::StartDocument { .. } => {
                    return Err(self.error(position, "UI layouts must use UTF-8"));
                }
                XmlEvent::StartElement {
                    name, attributes, ..
                } => {
                    if name.namespace.is_some() {
                        return Err(self.error(position, "widget elements must be unqualified"));
                    }
                    return Ok(Some(Tag {
                        name: name.local_name,
                        attributes,
                        position,
                    }));
                }
                XmlEvent::EndElement { .. } | XmlEvent::EndDocument => return Ok(None),
                XmlEvent::Whitespace(_) | XmlEvent::Comment(_) => {}
                _ => return Err(self.error(position, "expected widget markup")),
            }
        }
    }

    fn finish_attributes(&self, tag: &Tag) -> Result<(), LayoutError> {
        if let Some(attr) = tag.attributes.first() {
            return Err(self.error(
                tag.position,
                format!("unexpected attribute {} on <{}>", attr.name, tag.name),
            ));
        }
        Ok(())
    }

    fn finish_leaf(&mut self, tag: &Tag) -> Result<(), LayoutError> {
        self.finish_attributes(tag)?;
        if let Some(child) = self.child()? {
            return Err(self.error(
                child.position,
                format!("<{}> cannot contain widgets", tag.name),
            ));
        }
        Ok(())
    }

    fn required(&self, tag: &mut Tag, name: &str) -> Result<String, LayoutError> {
        tag.take(name)
            .ok_or_else(|| self.error(tag.position, format!("<{}> requires {name}", tag.name)))
    }

    fn optional_token(&self, tag: &mut Tag, name: &str) -> Result<Option<String>, LayoutError> {
        let value = tag.take(name);
        if let Some(value) = &value {
            self.check_token(tag, name, value)?;
        }
        Ok(value)
    }

    fn check_token(&self, tag: &Tag, attr: &str, value: &str) -> Result<(), LayoutError> {
        if value.is_empty() || value.chars().any(char::is_whitespace) {
            return Err(self.error(tag.position, format!("{attr} must be a nonempty token")));
        }
        Ok(())
    }

    fn reserve(
        &mut self,
        position: TextPosition,
        depth: usize,
        widgets: usize,
    ) -> Result<(), LayoutError> {
        if depth > MAX_DEPTH {
            return Err(self.error(position, format!("widget depth exceeds {MAX_DEPTH}")));
        }
        self.remaining = self.remaining.checked_sub(widgets).ok_or_else(|| {
            self.error(
                position,
                format!("layout exceeds the {MAX_WIDGETS} widget budget"),
            )
        })?;
        Ok(())
    }

    fn error(&self, position: TextPosition, message: impl Into<String>) -> LayoutError {
        LayoutError {
            source: self.source.to_owned(),
            line: position.row + 1,
            column: position.column + 1,
            message: message.into(),
        }
    }
}
