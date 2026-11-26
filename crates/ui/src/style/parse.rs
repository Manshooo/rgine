use std::fmt;

use super::{Declaration, Dimension, Edges, LineHeight, Rule, Selector, Simple, StyleSheet};

/// A stylesheet error at a one-based source line and column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleError {
    /// Stylesheet name supplied to [StyleSheet::parse].
    pub source: String,
    /// One-based line in the source.
    pub line: usize,
    /// One-based column in the source.
    pub column: usize,
    /// Description of the invalid rule or value.
    pub message: String,
}

impl fmt::Display for StyleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.source, self.line, self.column, self.message
        )
    }
}

impl std::error::Error for StyleError {}

pub(super) fn parse(source: &str, css: &str) -> Result<StyleSheet, StyleError> {
    let css = css.strip_prefix('\u{feff}').unwrap_or(css);
    let mut input = Parser {
        source,
        css,
        offset: 0,
    };
    let mut rules = Vec::new();
    while {
        input.space()?;
        input.peek().is_some()
    } {
        rules.push(input.rule()?);
    }
    Ok(StyleSheet { rules })
}

struct Parser<'a> {
    source: &'a str,
    css: &'a str,
    offset: usize,
}

impl<'a> Parser<'a> {
    fn rule(&mut self) -> Result<Rule, StyleError> {
        let mut selectors = vec![self.selector()?];
        while self.take(',') {
            self.space()?;
            selectors.push(self.selector()?);
        }
        self.expect('{')?;
        let mut declarations = Vec::new();
        loop {
            self.space()?;
            if self.take('}') {
                return Ok(Rule {
                    selectors,
                    declarations,
                });
            }
            if self.take(';') {
                continue;
            }
            let offset = self.offset;
            let property = self.ident()?.to_ascii_lowercase();
            self.space()?;
            self.expect(':')?;
            self.space()?;
            declarations.push(self.declaration(&property, offset)?);
            self.space()?;
            if !self.take(';') && self.peek() != Some('}') {
                return Err(self.error("expected ';' or '}' after the value"));
            }
        }
    }

    fn selector(&mut self) -> Result<Selector, StyleError> {
        let mut groups = vec![vec![self.compound()?]];
        loop {
            let spaced = self.space()?;
            if self.take('>') {
                self.space()?;
                groups.last_mut().unwrap().push(self.compound()?);
            } else if matches!(self.peek(), Some(',' | '{')) {
                break;
            } else if spaced && self.peek().is_some() {
                groups.push(vec![self.compound()?]);
            } else {
                return Err(self.error("expected a selector combinator, ',' or '{'"));
            }
        }
        let mut specificity = [0; 3];
        for simple in groups.iter().flatten().flatten() {
            for (total, weight) in specificity.iter_mut().zip(simple.specificity()) {
                *total += weight;
            }
        }
        Ok(Selector {
            groups,
            specificity,
        })
    }

    fn compound(&mut self) -> Result<Vec<Simple>, StyleError> {
        let mut compound = Vec::new();
        if self.take('*') {
            compound.push(Simple::Any);
        } else if self.peek().is_some_and(ident_start) {
            const TYPES: [(&str, &str); 4] = [
                ("panel", "panel"),
                ("label", "label"),
                ("image", "image"),
                ("button", "button"),
            ];
            compound.push(Simple::Type(self.keyword(&TYPES, "widget type")?));
        }
        loop {
            self.comments()?;
            let simple = match self.peek() {
                Some('#') => {
                    self.advance();
                    Simple::Id(self.ident()?.to_owned())
                }
                Some('.') => {
                    self.advance();
                    Simple::Class(self.ident()?.to_owned())
                }
                Some(':') => {
                    self.advance();
                    let offset = self.offset;
                    let name = self.ident()?;
                    match name.to_ascii_lowercase().as_str() {
                        "hover" => Simple::Hover,
                        "focus" => Simple::Focus,
                        "active" => Simple::Active,
                        "disabled" => Simple::Disabled,
                        _ => {
                            return Err(self.error_at(offset, format!("unsupported state :{name}")));
                        }
                    }
                }
                _ => break,
            };
            compound.push(simple);
        }
        if compound.is_empty() {
            return Err(self.error("expected a widget selector"));
        }
        Ok(compound)
    }

    fn declaration(&mut self, property: &str, offset: usize) -> Result<Declaration, StyleError> {
        use Declaration::*;
        Ok(match property {
            "width" => Width(self.dimension()?),
            "height" => Height(self.dimension()?),
            "min-width" => MinWidth(self.dimension()?),
            "max-width" => MaxWidth(self.dimension()?),
            "min-height" => MinHeight(self.dimension()?),
            "max-height" => MaxHeight(self.dimension()?),
            "margin" => Margin(self.edges()?),
            "padding" => Padding(self.edges()?),
            "gap" => Gap(self.pixels()?),
            "flex-direction" => Flow(self.keyword(
                &[("row", super::Flow::Row), ("column", super::Flow::Column)],
                "flow direction",
            )?),
            "align-items" => AlignItems(self.keyword(
                &[
                    ("flex-start", super::AlignItems::Start),
                    ("center", super::AlignItems::Center),
                    ("flex-end", super::AlignItems::End),
                    ("stretch", super::AlignItems::Stretch),
                ],
                "cross-axis alignment",
            )?),
            "justify-content" => JustifyContent(self.keyword(
                &[
                    ("flex-start", super::JustifyContent::Start),
                    ("center", super::JustifyContent::Center),
                    ("flex-end", super::JustifyContent::End),
                    ("space-between", super::JustifyContent::SpaceBetween),
                    ("space-around", super::JustifyContent::SpaceAround),
                    ("space-evenly", super::JustifyContent::SpaceEvenly),
                ],
                "main-axis distribution",
            )?),
            "background-color" => BackgroundColor(self.color()?),
            "color" => Color(self.color()?),
            "font-family" => FontFamilies(self.font_families()?),
            "font-size" => {
                let offset = self.offset;
                let size = self.pixels()?;
                if size == 0.0 {
                    return Err(self.error_at(offset, "font size must be positive"));
                }
                FontSize(size)
            }
            "font-weight" => FontWeight(self.font_weight()?),
            "line-height" => LineHeight(self.line_height()?),
            "text-align" => TextAlign(self.keyword(
                &[
                    ("left", super::TextAlign::Left),
                    ("center", super::TextAlign::Center),
                    ("right", super::TextAlign::Right),
                ],
                "text alignment",
            )?),
            _ => return Err(self.error_at(offset, format!("unsupported property {property:?}"))),
        })
    }

    fn dimension(&mut self) -> Result<Dimension, StyleError> {
        if self.peek().is_some_and(ident_start) && self.peek() != Some('-') {
            return self.keyword(&[("auto", Dimension::Auto)], "size keyword");
        }
        let offset = self.offset;
        let value = self.nonnegative()?;
        if self.take('%') {
            return Ok(Dimension::Percent(value));
        }
        self.px_unit(offset, value).map(Dimension::Px)
    }

    fn edges(&mut self) -> Result<Edges, StyleError> {
        let mut values = Vec::with_capacity(4);
        for _ in 0..4 {
            values.push(self.pixels()?);
            self.space()?;
            if matches!(self.peek(), Some(';' | '}') | None) {
                break;
            }
        }
        let (top, right, bottom, left) = match values.as_slice() {
            [all] => (*all, *all, *all, *all),
            [vertical, horizontal] => (*vertical, *horizontal, *vertical, *horizontal),
            [top, horizontal, bottom] => (*top, *horizontal, *bottom, *horizontal),
            [top, right, bottom, left] => (*top, *right, *bottom, *left),
            _ => unreachable!("parsed one through four edges"),
        };
        Ok(Edges {
            top,
            right,
            bottom,
            left,
        })
    }

    fn pixels(&mut self) -> Result<f32, StyleError> {
        let offset = self.offset;
        let value = self.nonnegative()?;
        self.px_unit(offset, value)
    }

    fn px_unit(&mut self, offset: usize, value: f32) -> Result<f32, StyleError> {
        if self.peek().is_some_and(ident_start) {
            self.keyword(&[("px", value)], "length unit")
        } else if value == 0.0 {
            Ok(value)
        } else {
            Err(self.error_at(offset, "expected a length in px"))
        }
    }

    fn color(&mut self) -> Result<[u8; 4], StyleError> {
        if !self.take('#') {
            return self.keyword(
                &[
                    ("transparent", [0; 4]),
                    ("black", [0, 0, 0, 255]),
                    ("white", [255; 4]),
                ],
                "color",
            );
        }
        let offset = self.offset - 1;
        let hex = self.take_while(|ch| ch.is_ascii_hexdigit());
        let mut rgba = [0, 0, 0, 255];
        let chars_per_channel = match hex.len() {
            3 | 4 => 1,
            6 | 8 => 2,
            _ => return Err(self.error_at(offset, "expected 3, 4, 6 or 8 hex digits")),
        };
        for (channel, start) in rgba
            .iter_mut()
            .zip((0..hex.len()).step_by(chars_per_channel))
        {
            let part = &hex[start..start + chars_per_channel];
            let value = u8::from_str_radix(part, 16).expect("validated hexadecimal digits");
            *channel = if chars_per_channel == 1 {
                value * 17
            } else {
                value
            };
        }
        Ok(rgba)
    }

    fn font_families(&mut self) -> Result<Vec<String>, StyleError> {
        let mut families = Vec::new();
        loop {
            let offset = self.offset;
            let family = match self.peek() {
                Some(quote @ ('"' | '\'')) => self.quoted(quote)?,
                _ => {
                    let mut family = self.ident()?.to_owned();
                    while self.space()? && self.peek().is_some_and(ident_start) {
                        family.push(' ');
                        family.push_str(self.ident()?);
                    }
                    family
                }
            };
            if family.trim().is_empty() {
                return Err(self.error_at(offset, "font family must have a name"));
            }
            families.push(family);
            self.space()?;
            if !self.take(',') {
                return Ok(families);
            }
            self.space()?;
        }
    }

    fn quoted(&mut self, quote: char) -> Result<String, StyleError> {
        let offset = self.offset;
        self.advance();
        let mut value = String::new();
        while let Some(ch) = self.advance() {
            match ch {
                ch if ch == quote => return Ok(value),
                '\\' => match self.advance() {
                    Some(ch) if ch == quote || ch == '\\' => value.push(ch),
                    _ => return Err(self.error_at(offset, "unsupported string escape")),
                },
                '\r' | '\n' | '\x0c' => break,
                ch => value.push(ch),
            }
        }
        Err(self.error_at(offset, "unterminated quoted font family"))
    }

    fn font_weight(&mut self) -> Result<u16, StyleError> {
        if self.peek().is_some_and(ident_start) && self.peek() != Some('-') {
            return self.keyword(&[("normal", 400), ("bold", 700)], "font weight");
        }
        let offset = self.offset;
        let value = self.number()?;
        if value.fract() != 0.0 || !(1.0..=1000.0).contains(&value) {
            return Err(self.error_at(offset, "font weight must be an integer from 1 through 1000"));
        }
        Ok(value as u16)
    }

    fn line_height(&mut self) -> Result<LineHeight, StyleError> {
        if self.peek().is_some_and(ident_start) && self.peek() != Some('-') {
            return self.keyword(&[("normal", LineHeight::Normal)], "line height");
        }
        let value = self.nonnegative()?;
        if self.peek().is_some_and(ident_start) {
            self.keyword(&[("px", LineHeight::Px(value))], "line height unit")
        } else {
            Ok(LineHeight::Factor(value))
        }
    }

    fn keyword<T: Copy>(
        &mut self,
        options: &[(&str, T)],
        description: &str,
    ) -> Result<T, StyleError> {
        let offset = self.offset;
        let value = self.ident()?;
        options
            .iter()
            .find(|(keyword, _)| value.eq_ignore_ascii_case(keyword))
            .map(|(_, value)| *value)
            .ok_or_else(|| self.error_at(offset, format!("unsupported {description} {value:?}")))
    }

    fn nonnegative(&mut self) -> Result<f32, StyleError> {
        let offset = self.offset;
        let value = self.number()?;
        if value < 0.0 {
            return Err(self.error_at(offset, "expected a nonnegative value"));
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<f32, StyleError> {
        let offset = self.offset;
        if matches!(self.peek(), Some('+' | '-')) {
            self.advance();
        }
        self.take_while(|c| c.is_ascii_digit());
        if self.take('.') {
            self.take_while(|c| c.is_ascii_digit());
        }
        self.css[offset..self.offset]
            .parse::<f32>()
            .ok()
            .filter(|number| number.is_finite())
            .ok_or_else(|| self.error_at(offset, "expected a finite number"))
    }

    fn ident(&mut self) -> Result<&'a str, StyleError> {
        if !self.peek().is_some_and(ident_start) {
            return Err(self.error("expected a CSS identifier"));
        }
        Ok(self.take_while(|c| ident_start(c) || c.is_ascii_digit()))
    }

    fn space(&mut self) -> Result<bool, StyleError> {
        let mut spaced = false;
        loop {
            spaced |= !self.take_while(|c| c.is_ascii_whitespace()).is_empty();
            if !self.comments()? {
                return Ok(spaced);
            }
        }
    }

    fn comments(&mut self) -> Result<bool, StyleError> {
        let initial = self.offset;
        while let Some(comment) = self.css[self.offset..].strip_prefix("/*") {
            let end = comment
                .find("*/")
                .ok_or_else(|| self.error("unterminated comment"))?;
            self.offset += end + 4;
        }
        Ok(self.offset != initial)
    }

    fn peek(&self) -> Option<char> {
        self.css[self.offset..].chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.offset += ch.len_utf8();
        Some(ch)
    }

    fn take(&mut self, expected: char) -> bool {
        if self.peek() != Some(expected) {
            return false;
        }
        self.advance();
        true
    }

    fn expect(&mut self, expected: char) -> Result<(), StyleError> {
        if self.take(expected) {
            Ok(())
        } else {
            Err(self.error(format!("expected {expected:?}")))
        }
    }

    fn take_while(&mut self, predicate: impl Fn(char) -> bool) -> &'a str {
        let start = self.offset;
        while self.peek().is_some_and(&predicate) {
            self.advance();
        }
        &self.css[start..self.offset]
    }

    fn error(&self, message: impl Into<String>) -> StyleError {
        self.error_at(self.offset, message)
    }

    fn error_at(&self, offset: usize, message: impl Into<String>) -> StyleError {
        let mut line = 1;
        let mut column = 1;
        let mut prefix = self.css[..offset].chars().peekable();
        while let Some(ch) = prefix.next() {
            if ch == '\r' && prefix.peek() == Some(&'\n') {
                prefix.next();
            }
            match ch {
                '\r' | '\n' | '\x0c' => {
                    line += 1;
                    column = 1;
                }
                _ => column += 1,
            }
        }
        StyleError {
            source: self.source.to_owned(),
            line,
            column,
            message: message.into(),
        }
    }
}

fn ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || matches!(ch, '-' | '_') || !ch.is_ascii() && !ch.is_whitespace()
}
