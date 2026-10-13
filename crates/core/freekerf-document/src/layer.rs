//! Colour layers and their cut settings.

use std::fmt;
use std::str::FromStr;

use freekerf_geometry::hatch::FillPattern;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::DocumentError;

/// An sRGB colour, written `#RRGGBB` (upper case, like LaserGRBL's layer keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Color {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

impl Color {
    /// Black, the SVG default.
    pub const BLACK: Self = Self::rgb(0, 0, 0);

    /// A colour from its components.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

impl FromStr for Color {
    type Err = DocumentError;

    /// Parses `#RRGGBB` or `#RGB` (any case).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || DocumentError::Invalid(format!("not a #RRGGBB colour: `{s}`"));
        let hex = s.strip_prefix('#').ok_or_else(bad)?;
        let digits: Vec<u8> = hex
            .chars()
            .map(|c| c.to_digit(16).and_then(|d| u8::try_from(d).ok()))
            .collect::<Option<_>>()
            .ok_or_else(bad)?;
        match digits.as_slice() {
            [r, g, b] => Ok(Self::rgb(r * 17, g * 17, b * 17)),
            [r1, r2, g1, g2, b1, b2] => Ok(Self::rgb(r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2)),
            _ => Err(bad()),
        }
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Color {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Color".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^#[0-9A-Fa-f]{6}$",
            "description": "sRGB colour `#RRGGBB`"
        })
    }
}

/// How the objects of a layer are processed (LaserGRBL's `SvgLayerMode`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LayerMode {
    /// Engrave the outlines.
    #[default]
    Line,
    /// Engrave the area inside the shapes with a hatch pattern.
    Fill,
    /// Fill, then engrave the outline for a clean edge.
    FillAndLine,
    /// Cut through the outlines; cut layers always run last.
    Cut,
    /// Skip the layer.
    Ignore,
}

impl LayerMode {
    /// The layer engraves a filling.
    #[must_use]
    pub const fn has_fill(self) -> bool {
        matches!(self, Self::Fill | Self::FillAndLine)
    }

    /// The layer follows the outlines.
    #[must_use]
    pub const fn has_outline(self) -> bool {
        matches!(self, Self::Line | Self::FillAndLine | Self::Cut)
    }

    /// Execution group: fillings first, then lines, cuts last (otherwise the piece may
    /// move before the engraving is done). Layers keep their order inside a group.
    #[must_use]
    pub const fn execution_order(self) -> u8 {
        match self {
            Self::Fill | Self::FillAndLine => 0,
            Self::Line => 1,
            Self::Cut | Self::Ignore => 2,
        }
    }

    /// LaserGRBL's name (`Line`, `Fill`, `FillAndLine`, `Cut`, `Ignore`).
    #[must_use]
    pub const fn lasergrbl_name(self) -> &'static str {
        match self {
            Self::Line => "Line",
            Self::Fill => "Fill",
            Self::FillAndLine => "FillAndLine",
            Self::Cut => "Cut",
            Self::Ignore => "Ignore",
        }
    }
}

/// Hatch filling of a layer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FillSettings {
    /// Pattern.
    pub pattern: FillPattern,
    /// Lines per millimetre (the distance between lines is `1 / lines_per_mm`).
    pub lines_per_mm: f64,
}

impl Default for FillSettings {
    fn default() -> Self {
        Self {
            pattern: FillPattern::Horizontal,
            lines_per_mm: 10.0,
        }
    }
}

/// Identifier of a layer inside a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct LayerId(pub u32);

/// How the laser is switched on for a layer's moves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LaserMode {
    /// The machine's laser on command (the planner's `laser_on`, `M3` by default).
    #[default]
    Machine,
    /// `M3`: constant power, also while the head accelerates and in corners.
    Constant,
    /// `M4`: dynamic power, scaled with the actual speed by Grbl in laser mode
    /// (`$32=1`), so corners and accelerations do not burn darker.
    Dynamic,
}

impl LaserMode {
    /// The laser on command of this mode; `None` for [`LaserMode::Machine`].
    #[must_use]
    pub const fn command(self) -> Option<&'static str> {
        match self {
            Self::Machine => None,
            Self::Constant => Some("M3"),
            Self::Dynamic => Some("M4"),
        }
    }

    const fn is_machine(&self) -> bool {
        matches!(self, Self::Machine)
    }
}

/// A colour layer: the objects of one colour and how to engrave or cut them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Layer {
    /// Identifier.
    pub id: LayerId,
    /// Colour that groups the objects.
    pub color: Color,
    /// Display name (the colour when empty).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Processing mode.
    #[serde(default)]
    pub mode: LayerMode,
    /// Feed rate while the laser is on, mm/min.
    pub speed_mm_min: u32,
    /// Laser power as the `S` value sent to the machine.
    pub power: u32,
    /// Number of passes (at least 1).
    #[serde(default = "one")]
    pub passes: u32,
    /// Filling (used by `fill` and `fill_and_line`).
    #[serde(default)]
    pub fill: FillSettings,
    /// Shown in the editor (hidden layers are still planned unless `ignore`).
    #[serde(default = "yes")]
    pub visible: bool,
    /// Laser on command (`M3` constant or `M4` dynamic power); by default the
    /// machine's.
    #[serde(default, skip_serializing_if = "LaserMode::is_machine")]
    pub laser_mode: LaserMode,
}

const fn one() -> u32 {
    1
}

const fn yes() -> bool {
    true
}

impl Layer {
    /// A layer with LaserGRBL's defaults (`Line`, 1 pass, horizontal filling at 10 lines/mm).
    #[must_use]
    pub fn new(id: LayerId, color: Color, speed_mm_min: u32, power: u32) -> Self {
        Self {
            id,
            color,
            name: String::new(),
            mode: LayerMode::Line,
            speed_mm_min,
            power,
            passes: 1,
            fill: FillSettings::default(),
            visible: true,
            laser_mode: LaserMode::Machine,
        }
    }

    /// Name to show: the name, or the colour.
    #[must_use]
    pub fn display_name(&self) -> String {
        if self.name.is_empty() {
            self.color.to_string()
        } else {
            self.name.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_parse_and_print() {
        assert_eq!("#ff0000".parse::<Color>().unwrap(), Color::rgb(255, 0, 0));
        assert_eq!("#0f0".parse::<Color>().unwrap(), Color::rgb(0, 255, 0));
        assert_eq!(Color::rgb(171, 205, 239).to_string(), "#ABCDEF");
        for bad in ["ff0000", "#ff00", "#gg0000", "#ff00000"] {
            assert!(bad.parse::<Color>().is_err(), "{bad}");
        }
        let json = serde_json::to_string(&Color::BLACK).unwrap();
        assert_eq!(json, "\"#000000\"");
        assert_eq!(serde_json::from_str::<Color>(&json).unwrap(), Color::BLACK);
        assert!(serde_json::from_str::<Color>("\"red\"").is_err());
    }

    #[test]
    fn modes_order_and_parts() {
        assert!(LayerMode::FillAndLine.has_fill() && LayerMode::FillAndLine.has_outline());
        assert!(!LayerMode::Fill.has_outline() && !LayerMode::Cut.has_fill());
        assert!(!LayerMode::Ignore.has_fill() && !LayerMode::Ignore.has_outline());
        assert!(LayerMode::Fill.execution_order() < LayerMode::Line.execution_order());
        assert!(LayerMode::Line.execution_order() < LayerMode::Cut.execution_order());
        assert_eq!(LayerMode::FillAndLine.lasergrbl_name(), "FillAndLine");
        for m in [
            LayerMode::Line,
            LayerMode::Fill,
            LayerMode::Cut,
            LayerMode::Ignore,
        ] {
            assert!(!m.lasergrbl_name().is_empty());
        }
    }

    #[test]
    fn layer_defaults_and_names() {
        let mut l = Layer::new(LayerId(1), Color::BLACK, 1000, 500);
        assert_eq!(l.display_name(), "#000000");
        l.name = "Cut".into();
        assert_eq!(l.display_name(), "Cut");
        let back: Layer =
            serde_json::from_str(r##"{"id":2,"color":"#123456","speed_mm_min":300,"power":1000}"##).unwrap();
        assert_eq!(back.passes, 1);
        assert!(back.visible);
        assert_eq!(back.mode, LayerMode::Line);
        assert_eq!(back.fill, FillSettings::default());
    }
}
