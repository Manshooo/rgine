use crate::{Widget, WidgetKind, WidgetTree};

mod parse;

pub use parse::StyleError;

/// Interaction flags supplied by the UI or game.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WidgetState {
    /// A pointer is over the widget.
    pub hovered: bool,
    /// The widget has keyboard focus.
    pub focused: bool,
    /// The widget is being activated, for example during a press.
    pub active: bool,
    /// The widget is disabled.
    pub disabled: bool,
}

/// A requested size in logical units.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum Dimension {
    /// Leave the size or constraint to layout.
    #[default]
    Auto,
    /// Logical pixels.
    Px(f32),
    /// Percentage of the available parent axis, where 100 fills it.
    Percent(f32),
}

/// Spacing in logical pixels.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Edges {
    /// Top edge.
    pub top: f32,
    /// Right edge.
    pub right: f32,
    /// Bottom edge.
    pub bottom: f32,
    /// Left edge.
    pub left: f32,
}

/// The primary axis for a panel's children.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Horizontal flow.
    #[default]
    Row,
    /// Vertical flow.
    Column,
}

/// Alignment across the flow axis.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum AlignItems {
    /// At the start of the axis.
    Start,
    /// Centered across the axis.
    Center,
    /// At the end of the axis.
    End,
    /// Fill the cross axis when size is automatic.
    #[default]
    Stretch,
}

/// Distribution along the flow axis.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum JustifyContent {
    /// At the start of the axis.
    #[default]
    Start,
    /// Centered on the axis.
    Center,
    /// At the end of the axis.
    End,
    /// Distribute extra space between children.
    SpaceBetween,
    /// Distribute extra space around children.
    SpaceAround,
    /// Distribute equal extra spaces, including at both ends.
    SpaceEvenly,
}

/// Line spacing supplied to text measurement.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum LineHeight {
    /// Use the font metrics.
    #[default]
    Normal,
    /// Multiple of the widget's font size.
    Factor(f32),
    /// Logical pixels.
    Px(f32),
}

/// Horizontal alignment of text within its box.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    /// Left aligned.
    #[default]
    Left,
    /// Centered.
    Center,
    /// Right aligned.
    Right,
}

/// Inherited text properties for labels and button captions.
#[derive(Debug, Clone, PartialEq)]
pub struct Typography {
    /// Project font families in fallback order. Empty uses the project default.
    pub font_families: Vec<String>,
    /// Font size in logical pixels.
    pub font_size: f32,
    /// CSS font weight from 1 through 1000.
    pub font_weight: u16,
    /// Requested line height.
    pub line_height: LineHeight,
    /// Horizontal text alignment.
    pub align: TextAlign,
    /// Text color as sRGB red, green, blue and linear alpha bytes.
    pub color: [u8; 4],
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            font_families: Vec::new(),
            font_size: 16.0,
            font_weight: 400,
            line_height: LineHeight::Normal,
            align: TextAlign::Left,
            color: [255; 4],
        }
    }
}

/// The styles available to layout and drawing after matching and inheritance.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ComputedStyle {
    /// Requested width.
    pub width: Dimension,
    /// Requested height.
    pub height: Dimension,
    /// Minimum width, automatic when unspecified.
    pub min_width: Dimension,
    /// Maximum width, automatic when unspecified.
    pub max_width: Dimension,
    /// Minimum height, automatic when unspecified.
    pub min_height: Dimension,
    /// Maximum height, automatic when unspecified.
    pub max_height: Dimension,
    /// Spacing outside the widget.
    pub margin: Edges,
    /// Spacing inside the widget.
    pub padding: Edges,
    /// Logical pixels between adjacent children.
    pub gap: f32,
    /// The panel's main axis.
    pub flow: Flow,
    /// How children align across the main axis.
    pub align_items: AlignItems,
    /// How children are distributed along the main axis.
    pub justify_content: JustifyContent,
    /// Background color as sRGB red, green, blue and linear alpha bytes.
    pub background_color: [u8; 4],
    /// Inherited text properties.
    pub typography: Typography,
}

/// A widget with its computed style and styled children in document order.
#[derive(Debug)]
pub struct StyledWidget<'a> {
    /// The authored widget.
    pub widget: &'a Widget,
    /// Effective style for the widget and its current state.
    pub style: ComputedStyle,
    /// The independently styled children of this widget instance.
    pub children: Vec<StyledWidget<'a>>,
}

/// A parsed game UI stylesheet using the engine's supported CSS subset.
#[derive(Debug, Default)]
pub struct StyleSheet {
    rules: Vec<Rule>,
}

impl StyleSheet {
    /// Parses an entire stylesheet, with diagnostics named by `source`.
    pub fn parse(source: &str, css: &str) -> Result<Self, StyleError> {
        parse::parse(source, css)
    }

    /// Resolves styles for a tree and the supplied state of each widget instance.
    ///
    /// The callback receives the child-index path from the root. The root's path
    /// is empty. Re-run this after a state change to update affected styles.
    pub fn resolve<'a>(
        &self,
        tree: &'a WidgetTree,
        mut state: impl FnMut(&[usize], &Widget) -> WidgetState,
    ) -> StyledWidget<'a> {
        Resolve {
            sheet: self,
            state: &mut state,
            indices: Vec::new(),
            nodes: Vec::new(),
        }
        .widget(tree.root(), &Typography::default())
    }
}

struct Resolve<'a, 'w, F> {
    sheet: &'a StyleSheet,
    state: &'a mut F,
    indices: Vec<usize>,
    nodes: Vec<MatchNode<'w>>,
}

impl<'w, F: FnMut(&[usize], &Widget) -> WidgetState> Resolve<'_, 'w, F> {
    fn widget(&mut self, widget: &'w Widget, inherited: &Typography) -> StyledWidget<'w> {
        self.nodes.push(MatchNode {
            widget,
            state: (self.state)(&self.indices, widget),
        });
        let mut matched = Vec::new();
        for (order, rule) in self.sheet.rules.iter().enumerate() {
            let specificity = rule
                .selectors
                .iter()
                .filter(|selector| selector.matches(&self.nodes))
                .map(|selector| selector.specificity)
                .max();
            if let Some(specificity) = specificity {
                matched.push((specificity, order, &rule.declarations));
            }
        }
        matched.sort_unstable_by_key(|(specificity, order, _)| (*specificity, *order));
        let mut style = ComputedStyle {
            typography: inherited.clone(),
            ..Default::default()
        };
        for (_, _, declarations) in matched {
            for declaration in declarations {
                declaration.apply(&mut style);
            }
        }
        let mut children = Vec::with_capacity(widget.children().len());
        for (index, child) in widget.children().iter().enumerate() {
            self.indices.push(index);
            children.push(self.widget(child, &style.typography));
            self.indices.pop();
        }
        self.nodes.pop();
        StyledWidget {
            widget,
            style,
            children,
        }
    }
}

#[derive(Debug)]
struct Rule {
    selectors: Vec<Selector>,
    declarations: Vec<Declaration>,
}

#[derive(Debug)]
struct Selector {
    // Descendant relationships separate groups. Inside a group each compound
    // must match the immediate child of the previous compound's widget.
    groups: Vec<Vec<Vec<Simple>>>,
    specificity: [usize; 3],
}

impl Selector {
    fn matches(&self, nodes: &[MatchNode<'_>]) -> bool {
        let mut end = nodes.len();
        for (index, group) in self.groups.iter().rev().enumerate() {
            loop {
                let Some(start) = end.checked_sub(group.len()) else {
                    return false;
                };
                if group
                    .iter()
                    .zip(&nodes[start..end])
                    .all(|(compound, node)| compound.iter().all(|simple| simple.matches(node)))
                {
                    end = start;
                    break;
                }
                if index == 0 {
                    return false;
                }
                end -= 1;
            }
        }
        true
    }
}

struct MatchNode<'a> {
    widget: &'a Widget,
    state: WidgetState,
}

#[derive(Debug)]
enum Simple {
    Any,
    Type(&'static str),
    Id(String),
    Class(String),
    Hover,
    Focus,
    Active,
    Disabled,
}

impl Simple {
    fn matches(&self, node: &MatchNode<'_>) -> bool {
        match self {
            Self::Any => true,
            Self::Type(name) => {
                *name
                    == match node.widget.kind() {
                        WidgetKind::Panel => "panel",
                        WidgetKind::Label { .. } => "label",
                        WidgetKind::Image { .. } => "image",
                        WidgetKind::Button { .. } => "button",
                    }
            }
            Self::Id(id) => node.widget.id() == Some(id),
            Self::Class(class) => node.widget.classes().contains(class),
            Self::Hover => node.state.hovered,
            Self::Focus => node.state.focused,
            Self::Active => node.state.active,
            Self::Disabled => node.state.disabled,
        }
    }

    fn specificity(&self) -> [usize; 3] {
        match self {
            Self::Any => [0, 0, 0],
            Self::Id(_) => [1, 0, 0],
            Self::Type(_) => [0, 0, 1],
            _ => [0, 1, 0],
        }
    }
}

#[derive(Debug)]
enum Declaration {
    Width(Dimension),
    Height(Dimension),
    MinWidth(Dimension),
    MaxWidth(Dimension),
    MinHeight(Dimension),
    MaxHeight(Dimension),
    Margin(Edges),
    Padding(Edges),
    Gap(f32),
    Flow(Flow),
    AlignItems(AlignItems),
    JustifyContent(JustifyContent),
    BackgroundColor([u8; 4]),
    Color([u8; 4]),
    FontFamilies(Vec<String>),
    FontSize(f32),
    FontWeight(u16),
    LineHeight(LineHeight),
    TextAlign(TextAlign),
}

impl Declaration {
    fn apply(&self, style: &mut ComputedStyle) {
        match self {
            Self::Width(value) => style.width = *value,
            Self::Height(value) => style.height = *value,
            Self::MinWidth(value) => style.min_width = *value,
            Self::MaxWidth(value) => style.max_width = *value,
            Self::MinHeight(value) => style.min_height = *value,
            Self::MaxHeight(value) => style.max_height = *value,
            Self::Margin(value) => style.margin = *value,
            Self::Padding(value) => style.padding = *value,
            Self::Gap(value) => style.gap = *value,
            Self::Flow(value) => style.flow = *value,
            Self::AlignItems(value) => style.align_items = *value,
            Self::JustifyContent(value) => style.justify_content = *value,
            Self::BackgroundColor(value) => style.background_color = *value,
            Self::Color(value) => style.typography.color = *value,
            Self::FontFamilies(value) => style.typography.font_families.clone_from(value),
            Self::FontSize(value) => style.typography.font_size = *value,
            Self::FontWeight(value) => style.typography.font_weight = *value,
            Self::LineHeight(value) => style.typography.line_height = *value,
            Self::TextAlign(value) => style.typography.align = *value,
        }
    }
}
