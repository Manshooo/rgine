//! Logical widget geometry for drawing and hit testing.

use std::fmt;

use taffy::geometry::{Rect as EdgesBox, Size};
use taffy::prelude::TaffyAuto;
use taffy::style::{self, AvailableSpace, FlexDirection, LengthPercentage, LengthPercentageAuto};
use taffy::{NodeId, Style, TaffyError, TaffyTree};

use crate::text::{FontBook, TextError, TextLayout};
use crate::{
    AlignItems, ComputedStyle, Dimension, Edges, Flow, JustifyContent, StyledWidget, TextAlign,
    WidgetKind,
};

/// An axis-aligned rectangle in viewport coordinates, with a top-left origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// The included top-left corner.
    pub origin: [f32; 2],
    /// Width and height. The right and bottom edges are excluded.
    pub size: [f32; 2],
}

impl Rect {
    /// Returns whether a point falls inside the half-open rectangle.
    pub fn contains(self, [x, y]: [f32; 2]) -> bool {
        x >= self.origin[0]
            && y >= self.origin[1]
            && x < self.origin[0] + self.size[0]
            && y < self.origin[1] + self.size[1]
    }

    /// Scales a logical rectangle to physical pixels without rounding or reflow.
    pub fn physical(self, display_scale: f32) -> Self {
        Self {
            origin: self.origin.map(|axis| axis * display_scale),
            size: self.size.map(|axis| axis * display_scale),
        }
    }
}

/// The shaped runs and their placement inside a label or button.
#[derive(Debug)]
pub struct TextPlacement {
    /// Logical viewport position to add to the prepared glyph positions.
    pub origin: [f32; 2],
    /// The measured text at the widget's final content width.
    pub layout: TextLayout,
}

/// Layout of one widget instance. Children retain authored order for drawing.
#[derive(Debug)]
pub struct WidgetBox<'style, 'tree> {
    /// The authored widget and its resolved style.
    pub styled: &'style StyledWidget<'tree>,
    /// Logical outer box, including padding and excluding margin.
    pub bounds: Rect,
    /// Logical box available to the content after subtracting padding.
    pub content: Rect,
    /// Prepared text for labels and buttons.
    pub text: Option<TextPlacement>,
    /// Positioned children in document order.
    pub children: Vec<WidgetBox<'style, 'tree>>,
}

/// A failure to resolve widget geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GeometryError {
    /// The viewport is negative or nonfinite.
    InvalidViewport,
    /// A widget's style or computed geometry has an invalid numeric value.
    InvalidWidget(String),
    /// An image asset supplied invalid intrinsic dimensions.
    InvalidImage(String),
    /// A label or button could not be measured.
    Text { widget: String, source: TextError },
    /// The layout backend could not construct or compute the tree.
    Layout(String),
}

impl fmt::Display for GeometryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidViewport => formatter.write_str("invalid logical UI viewport"),
            Self::InvalidWidget(widget) => write!(formatter, "{widget}: invalid widget geometry"),
            Self::InvalidImage(path) => write!(formatter, "{path}: invalid intrinsic image size"),
            Self::Text { widget, source } => write!(formatter, "{widget}: {source}"),
            Self::Layout(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for GeometryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Text { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn layout_error(error: TaffyError) -> GeometryError {
    GeometryError::Layout(error.to_string())
}

/// Resolves a styled tree in a logical viewport. An automatic root axis fills
/// the viewport after its margins. Image sizes are logical intrinsic sizes
/// supplied by the caller. Missing image sizes contribute zero intrinsic size.
pub fn layout<'style, 'tree>(
    root: &'style StyledWidget<'tree>,
    viewport: [f32; 2],
    fonts: &FontBook,
    mut image_size: impl FnMut(&str) -> Option<[f32; 2]>,
) -> Result<WidgetBox<'style, 'tree>, GeometryError> {
    if !viewport.into_iter().all(valid_extent) {
        return Err(GeometryError::InvalidViewport);
    }
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let root_id = insert(&mut tree, root, Some(viewport), &mut image_size)?;
    let mut failure = None;
    tree.compute_layout_with_measure(
        root_id,
        axes(viewport).map(AvailableSpace::Definite),
        |inputs, _, context, style| {
            taffy::compute_leaf_layout(
                inputs,
                style,
                |_, _| 0.0,
                |_, available| {
                    let Some(context) = context else {
                        return Size::ZERO;
                    };
                    match context.measure(fonts, available.width) {
                        Ok(size) => axes(size),
                        Err(error) => {
                            failure.get_or_insert(error);
                            Size::ZERO
                        }
                    }
                },
            )
        },
    )
    .map_err(layout_error)?;
    if let Some(error) = failure {
        return Err(error);
    }
    collect(&mut tree, root_id, [0.0; 2], fonts)
}

struct Node<'style, 'tree> {
    styled: &'style StyledWidget<'tree>,
    intrinsic: [f32; 2],
    text: Option<(Option<f32>, TextLayout)>,
}

impl Node<'_, '_> {
    fn measure(
        &mut self,
        fonts: &FontBook,
        width: AvailableSpace,
    ) -> Result<[f32; 2], GeometryError> {
        let width = match width {
            AvailableSpace::Definite(width) => Some(width.max(0.0)),
            AvailableSpace::MinContent => Some(0.0),
            AvailableSpace::MaxContent => None,
        };
        let intrinsic = self.intrinsic;
        Ok(self
            .prepare_text(fonts, width)?
            .map_or(intrinsic, |text| text.size))
    }

    fn prepare_text(
        &mut self,
        fonts: &FontBook,
        width: Option<f32>,
    ) -> Result<Option<&TextLayout>, GeometryError> {
        let text = match self.styled.widget.kind() {
            WidgetKind::Label { text } | WidgetKind::Button { text } => text,
            _ => return Ok(None),
        };
        if !self
            .text
            .as_ref()
            .is_some_and(|(measured_width, _)| *measured_width == width)
        {
            let layout = fonts
                .prepare(text, &self.styled.style.typography, width)
                .map_err(|source| GeometryError::Text {
                    widget: widget_name(self.styled),
                    source,
                })?;
            self.text = Some((width, layout));
        }
        Ok(self.text.as_ref().map(|(_, layout)| layout))
    }
}

fn insert<'style, 'tree>(
    tree: &mut TaffyTree<Node<'style, 'tree>>,
    styled: &'style StyledWidget<'tree>,
    viewport: Option<[f32; 2]>,
    image_size: &mut impl FnMut(&str) -> Option<[f32; 2]>,
) -> Result<NodeId, GeometryError> {
    let style = box_style(&styled.style, viewport)
        .ok_or_else(|| GeometryError::InvalidWidget(widget_name(styled)))?;
    let intrinsic = if let WidgetKind::Image { src } = styled.widget.kind() {
        let size = image_size(src).unwrap_or([0.0; 2]);
        if !size.into_iter().all(valid_extent) {
            return Err(GeometryError::InvalidImage(src.clone()));
        }
        size
    } else {
        [0.0; 2]
    };
    let context = Node {
        styled,
        intrinsic,
        text: None,
    };
    if styled.children.is_empty() {
        return tree
            .new_leaf_with_context(style, context)
            .map_err(layout_error);
    }
    let children = styled
        .children
        .iter()
        .map(|child| insert(tree, child, None, image_size))
        .collect::<Result<Vec<_>, _>>()?;
    let node = tree
        .new_with_children(style, &children)
        .map_err(layout_error)?;
    tree.set_node_context(node, Some(context))
        .map_err(layout_error)?;
    Ok(node)
}

fn collect<'style, 'tree>(
    tree: &mut TaffyTree<Node<'style, 'tree>>,
    node: NodeId,
    parent: [f32; 2],
    fonts: &FontBook,
) -> Result<WidgetBox<'style, 'tree>, GeometryError> {
    let layout = tree.layout(node).map_err(layout_error)?;
    let origin = [parent[0] + layout.location.x, parent[1] + layout.location.y];
    let bounds = Rect {
        origin,
        size: [layout.size.width, layout.size.height],
    };
    let content = Rect {
        origin: [
            origin[0] + layout.padding.left,
            origin[1] + layout.padding.top,
        ],
        size: [
            (layout.size.width - layout.padding.left - layout.padding.right).max(0.0),
            (layout.size.height - layout.padding.top - layout.padding.bottom).max(0.0),
        ],
    };
    let context = tree
        .get_node_context_mut(node)
        .expect("every widget has a layout node context");
    let styled = context.styled;
    if !bounds
        .size
        .into_iter()
        .chain(content.size)
        .all(valid_extent)
        || !bounds
            .origin
            .into_iter()
            .chain(content.origin)
            .all(f32::is_finite)
    {
        return Err(GeometryError::InvalidWidget(widget_name(styled)));
    }
    context.prepare_text(fonts, Some(content.size[0]))?;
    let text = context.text.take().map(|(_, layout)| {
        let free = (content.size[0] - layout.size[0]).max(0.0);
        let x = match styled.style.typography.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => free * 0.5,
            TextAlign::Right => free,
        };
        TextPlacement {
            origin: [content.origin[0] + x, content.origin[1]],
            layout,
        }
    });
    let children = tree
        .children(node)
        .map_err(layout_error)?
        .into_iter()
        .map(|child| collect(tree, child, origin, fonts))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(WidgetBox {
        styled,
        bounds,
        content,
        text,
        children,
    })
}

fn box_style(style: &ComputedStyle, viewport: Option<[f32; 2]>) -> Option<Style> {
    let padding = edges(style.padding);
    let margin = edges(style.margin);
    let valid = [
        padding.left,
        padding.right,
        padding.top,
        padding.bottom,
        margin.left,
        margin.right,
        margin.top,
        margin.bottom,
        style.gap,
    ]
    .into_iter()
    .all(valid_extent);
    if !valid {
        return None;
    }
    let root_axis = |dimension, available: f32, leading: f32, trailing: f32| match dimension {
        Dimension::Auto => Dimension::Px((available - leading - trailing).max(0.0_f32)),
        fixed => fixed,
    };
    let [width, height] = viewport.map_or([style.width, style.height], |[width, height]| {
        [
            root_axis(style.width, width, margin.left, margin.right),
            root_axis(style.height, height, margin.top, margin.bottom),
        ]
    });
    Some(Style {
        display: taffy::Display::Flex,
        box_sizing: taffy::BoxSizing::BorderBox,
        size: Size {
            width: dimension(width)?.into(),
            height: dimension(height)?.into(),
        },
        min_size: Size {
            width: dimension(style.min_width)?,
            height: dimension(style.min_height)?,
        },
        max_size: Size {
            width: dimension(style.max_width)?,
            height: dimension(style.max_height)?,
        },
        margin: margin.map(LengthPercentageAuto::length),
        padding: padding.map(LengthPercentage::length),
        gap: Size {
            width: LengthPercentage::length(style.gap),
            height: LengthPercentage::length(style.gap),
        },
        flex_direction: match style.flow {
            Flow::Row => FlexDirection::Row,
            Flow::Column => FlexDirection::Column,
        },
        align_items: match style.align_items {
            AlignItems::Start => style::AlignItems::FLEX_START,
            AlignItems::Center => style::AlignItems::CENTER,
            AlignItems::End => style::AlignItems::FLEX_END,
            AlignItems::Stretch => style::AlignItems::STRETCH,
        },
        justify_content: match style.justify_content {
            JustifyContent::Start => style::JustifyContent::FLEX_START,
            JustifyContent::Center => style::JustifyContent::CENTER,
            JustifyContent::End => style::JustifyContent::FLEX_END,
            JustifyContent::SpaceBetween => style::JustifyContent::SPACE_BETWEEN,
            JustifyContent::SpaceAround => style::JustifyContent::SPACE_AROUND,
            JustifyContent::SpaceEvenly => style::JustifyContent::SPACE_EVENLY,
        },
        ..Default::default()
    })
}

fn dimension(dimension: Dimension) -> Option<LengthPercentageAuto> {
    match dimension {
        Dimension::Auto => Some(LengthPercentageAuto::AUTO),
        Dimension::Px(value) if valid_extent(value) => Some(LengthPercentageAuto::length(value)),
        Dimension::Percent(value) if valid_extent(value) => {
            Some(LengthPercentageAuto::percent(value / 100.0))
        }
        _ => None,
    }
}

fn valid_extent(value: f32) -> bool {
    value.is_finite() && value >= 0.0
}

fn edges(edges: Edges) -> EdgesBox<f32> {
    EdgesBox {
        left: edges.left,
        right: edges.right,
        top: edges.top,
        bottom: edges.bottom,
    }
}

fn axes<T>([width, height]: [T; 2]) -> Size<T> {
    Size { width, height }
}

fn widget_name(styled: &StyledWidget<'_>) -> String {
    styled.widget.id().map_or_else(
        || match styled.widget.kind() {
            WidgetKind::Panel => "panel".into(),
            WidgetKind::Label { .. } => "label".into(),
            WidgetKind::Button { .. } => "button".into(),
            WidgetKind::Image { src } => src.clone(),
        },
        |id| format!("#{id}"),
    )
}
