//! Shared look of the scoreboard and report cards: dark navy panels, gold chrome and teal accents.
use crate::render::{Canvas, escape};
use std::fmt::Write;

pub(crate) const BG: &str = "#010a13";
pub(crate) const GOLD: &str = "#c8aa6e";
pub(crate) const GOLD_DIM: &str = "#785a28";
pub(crate) const CREAM: &str = "#f0e6d2";
pub(crate) const TEAL: &str = "#0ac8b9";
pub(crate) const BLUE: &str = "#3c9bff";
pub(crate) const RED: &str = "#e84057";
pub(crate) const SOFT: &str = "#a09b8c";
pub(crate) const PANEL: &str = "#0c1d2e";
pub(crate) const PANEL_DARK: &str = "#071420";
pub(crate) const ROW_A: &str = "#0a1623";
pub(crate) const ROW_B: &str = "#08111c";
pub(crate) const RULE: &str = "#1e2328";

/// Tints of a team colour used for bars and labels: (light ink, bar gradient start).
pub(crate) fn team_ink(team: u16) -> &'static str { if team == 100 { "#7cb8ff" } else { "#ff8fa0" } }
pub(crate) fn team_color(team: u16) -> &'static str { if team == 100 { BLUE } else { RED } }

impl Canvas {
    fn next_id(&mut self, prefix: &str) -> String { self.clips += 1; format!("{prefix}{}", self.clips) }
    /// Registers a linear gradient (x1,y1 → x2,y2 in 0..1 box units) and returns its `url(#id)` paint.
    pub fn gradient(&mut self, stops: &[(f32, &str, f32)], line: (f32, f32, f32, f32)) -> String {
        let id = self.next_id("g");
        let stops: String = stops.iter().map(|(o, c, a)| format!("<stop offset='{o}' stop-color='{c}' stop-opacity='{a}'/>")).collect();
        let _ = write!(self.svg, "<defs><linearGradient id='{id}' x1='{}' y1='{}' x2='{}' y2='{}'>{stops}</linearGradient></defs>", line.0, line.1, line.2, line.3);
        format!("url(#{id})")
    }
    /// Rectangle with float geometry; `extra` carries any further attributes.
    pub fn block(&mut self, x: f32, y: f32, w: f32, h: f32, rx: f32, fill: &str, extra: &str) {
        let _ = write!(self.svg, "<rect x='{x}' y='{y}' width='{w}' height='{h}' rx='{rx}' fill='{fill}' {extra}/>");
    }
    pub fn stroke(&mut self, x: f32, y: f32, w: f32, h: f32, rx: f32, ink: &str, width: f32, opacity: f32) {
        let _ = write!(self.svg, "<rect x='{x}' y='{y}' width='{w}' height='{h}' rx='{rx}' fill='none' stroke='{ink}' stroke-width='{width}' stroke-opacity='{opacity}'/>");
    }
    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, ink: &str, width: f32) {
        let _ = write!(self.svg, "<line x1='{x1}' y1='{y1}' x2='{x2}' y2='{y2}' stroke='{ink}' stroke-width='{width}'/>");
    }
    pub fn polygon(&mut self, points: &str, fill: &str) { let _ = write!(self.svg, "<polygon points='{points}' fill='{fill}'/>"); }
    /// Text with float position, letter spacing and a free-form paint (colour or gradient).
    pub fn label(&mut self, x: f32, y: f32, size: f32, fill: &str, weight: u16, anchor: &str, spacing: f32, text: &str) {
        let _ = write!(self.svg, "<text x='{x}' y='{y}' font-size='{size}' fill='{fill}' font-weight='{weight}' text-anchor='{anchor}' letter-spacing='{spacing}'>{}</text>", escape(text));
    }
    /// A run of text with a dim tail: `head` in `ink` (bold) then `tail` in `tail_ink`.
    pub fn runs(&mut self, x: f32, y: f32, size: f32, anchor: &str, parts: &[(&str, &str, u16)]) {
        let spans: String = parts.iter().map(|(text, ink, weight)| format!("<tspan fill='{ink}' font-weight='{weight}'>{}</tspan>", escape(text))).collect();
        let _ = write!(self.svg, "<text x='{x}' y='{y}' font-size='{size}' text-anchor='{anchor}'>{spans}</text>");
    }
    /// Picture with float geometry. `fit` is an SVG preserveAspectRatio value ("xMidYMid slice" fills, "xMidYMid meet" fits).
    pub fn picture(&mut self, data: Option<&str>, x: f32, y: f32, w: f32, h: f32, rx: f32, fit: &str) {
        let Some(data) = data else { self.block(x, y, w, h, rx, "#101c2b", ""); return; };
        let id = self.next_id("c");
        let _ = write!(self.svg, "<defs><clipPath id='{id}'><rect x='{x}' y='{y}' width='{w}' height='{h}' rx='{rx}'/></clipPath></defs><image href='{data}' x='{x}' y='{y}' width='{w}' height='{h}' preserveAspectRatio='{fit}' clip-path='url(#{id})'/>");
    }
    /// Gold hairline that fades out toward both ends.
    pub fn hairline(&mut self, x1: f32, x2: f32, y: f32, opacity: f32) {
        let paint = self.gradient(&[(0.0, GOLD, 0.0), (0.5, GOLD, opacity), (1.0, GOLD, 0.0)], (0.0, 0.0, 1.0, 0.0));
        self.block(x1, y, x2 - x1, 1.0, 0.0, &paint, "");
    }
    /// Double gold frame around a portrait; `ink` replaces the outer line for badge winners.
    pub fn frame(&mut self, x: f32, y: f32, w: f32, h: f32, ink: &str) {
        self.stroke(x - 2.0, y - 2.0, w + 4.0, h + 4.0, 0.0, ink, 1.5, 1.0);
        self.stroke(x - 4.0, y - 4.0, w + 8.0, h + 8.0, 0.0, GOLD_DIM, 0.8, 1.0);
    }
    /// Small filled pill with an up or down triangle and a value, e.g. an LP change.
    pub fn delta_pill(&mut self, x: f32, y: f32, delta: i64, label: &str) {
        let up = delta > 0;
        let (ink, fill) = if up { (TEAL, "#0a3a35") } else { ("#ff7b8f", "#3d1a24") };
        self.block(x, y, 62.0, 18.0, 9.0, fill, &format!("stroke='{ink}' stroke-opacity='.55' stroke-width='1'"));
        let (a, b, c) = if up { ((x + 10.0, y + 6.0), (x + 15.0, y + 13.0), (x + 5.0, y + 13.0)) } else { ((x + 10.0, y + 13.0), (x + 15.0, y + 6.0), (x + 5.0, y + 6.0)) };
        self.polygon(&format!("{},{} {},{} {},{}", a.0, a.1, b.0, b.1, c.0, c.1), ink);
        self.label(x + 23.0, y + 13.0, 11.0, ink, 700, "start", 0.0, label);
    }
}

/// "12.3k" style short number.
pub(crate) fn thousands(value: i64) -> String { format!("{:.1}k", value as f64 / 1000.0) }
