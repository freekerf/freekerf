//! The vector G-code writer of LaserGRBL (`SvgConverter/gcodeRelated.cs`, from
//! GRBL-Plotter by Sven Hasemann): pen up/down as laser power changes, moves with
//! modal compression (an axis, `G` or `F` is written only when it changes), arcs as
//! `G2`/`G3` with `I`/`J`, or split into short lines.
//!
//! The state is single precision (`float` in C#) and numbers are formatted with
//! `"0.###"` on floats, so the text matches LaserGRBL's to the last digit.
//! Comments (`(…)`) are written like LaserGRBL's and removed when the text is loaded
//! as a program; they still leave the space before them (`S0 ` for a pen up).

use freekerf_gcode::dotnet::{single_format, single_to_string};
use freekerf_geometry::Point;

use crate::{Firmware, MachineOptions};

const NUMBER_FORMAT: &str = "0.###";

/// Writer state (the static fields of `gcode`).
#[derive(Debug, Clone)]
pub struct Writer {
    out: Vec<String>,
    firmware: Firmware,
    support_pwm: bool,
    rapid: u8,
    xy_feed: f32,
    spindle_speed: f32,
    cmd_on: String,
    cmd_off: String,
    apply_xy_feed: bool,
    last_x: f32,
    last_y: f32,
    last_g: f32,
    last_f: f32,
    last_s: f32,
}

#[allow(clippy::cast_precision_loss)]
fn f32_of(v: u32) -> f32 {
    v as f32
}

impl Writer {
    /// `gcode.setup()` + `setRapidNum()` for a new conversion: feed and power from the
    /// raster dialog defaults of `machine` (border speed, maximum power; the board's
    /// `$30` without PWM; divided by 255 for Smoothie).
    #[must_use]
    pub fn new(machine: &MachineOptions) -> Self {
        let mut spindle_speed = if machine.support_pwm {
            f32_of(machine.default_power_max)
        } else {
            f32_of(machine.max_pwm)
        };
        if machine.firmware == Firmware::Smoothie {
            spindle_speed /= 255.0;
        }
        Self {
            out: Vec::new(),
            firmware: machine.firmware,
            support_pwm: machine.support_pwm,
            rapid: u8::from(machine.disable_g0),
            xy_feed: f32_of(machine.default_border_speed),
            spindle_speed,
            cmd_on: machine.laser_on.clone(),
            cmd_off: machine.laser_off.clone(),
            apply_xy_feed: true,
            last_x: -1.0,
            last_y: -1.0,
            last_g: -1.0,
            last_f: 0.0,
            last_s: -1.0,
        }
    }

    /// The lines written so far.
    #[must_use]
    pub fn lines(&self) -> &[String] {
        &self.out
    }

    /// Takes the lines.
    #[must_use]
    pub fn into_lines(self) -> Vec<String> {
        self.out
    }

    /// Writes a line as it is (comments, custom code).
    pub fn raw(&mut self, line: impl Into<String>) {
        self.out.push(line.into());
    }

    fn comment(cmt: &str) -> String {
        if cmt.is_empty() {
            String::new()
        } else {
            format!(" ({cmt})")
        }
    }

    /// `SetLayerParams`: feed and power of the following moves (power divided by 255
    /// for Smoothie). Without PWM the power is written now (`S…`), since pen down
    /// then only switches the laser on.
    pub fn set_layer_params(&mut self, feed: u32, power: u32) {
        self.xy_feed = f32_of(feed);
        self.spindle_speed = f32_of(power);
        if self.firmware == Firmware::Smoothie {
            self.spindle_speed /= 255.0;
        }
        self.apply_xy_feed = true;
        if !self.support_pwm {
            let s = single_to_string(self.spindle_speed);
            self.out.push(format!("S{s}"));
        }
    }

    /// Switches the laser on command (`M3`/`M4`) for the following moves (a layer's
    /// laser mode). With PWM the laser stays on between moves, so the new command is
    /// written now when `write`; without PWM pen down writes it.
    pub fn set_laser_on(&mut self, cmd: &str, write: bool) {
        if cmd == self.cmd_on {
            return;
        }
        cmd.clone_into(&mut self.cmd_on);
        if write && self.support_pwm {
            self.out.push(cmd.to_owned());
        }
    }

    /// `PutInitialCommand`: laser on at zero power (PWM) or off with the power set.
    pub fn initial(&mut self) {
        if self.support_pwm {
            let l = format!("{} S0", self.cmd_on);
            self.out.push(l);
        } else {
            let l = format!("{} S{}", self.cmd_off, single_to_string(self.spindle_speed));
            self.out.push(l);
        }
    }

    /// `PutFinalCommand`: laser off.
    pub fn final_command(&mut self) {
        self.out.push("M5 S0".into());
    }

    /// `PenDown` / `SpindleOn`.
    pub fn pen_down(&mut self, cmt: &str) {
        self.apply_xy_feed = true;
        let c = Self::comment(cmt);
        let line = if self.support_pwm {
            format!("S{}{c}", single_to_string(self.spindle_speed))
        } else {
            format!("{}{c}", self.cmd_on)
        };
        self.out.push(line);
    }

    /// `PenUp` / `SpindleOff`.
    pub fn pen_up(&mut self, cmt: &str) {
        let c = Self::comment(cmt);
        let line = if self.support_pwm {
            format!("S0{c}")
        } else {
            format!("{}{c}", self.cmd_off)
        };
        self.out.push(line);
    }

    /// `MoveTo`: a `G1` to `p` (cast to single precision).
    #[allow(clippy::cast_possible_truncation)]
    pub fn move_to(&mut self, p: Point, cmt: &str) {
        let apply = self.apply_xy_feed;
        self.mv(1, p.x as f32, p.y as f32, apply, cmt);
    }

    /// `MoveToRapid`: a `G0` (or `G1` when G0 is disabled) to `p`.
    #[allow(clippy::cast_possible_truncation)]
    pub fn move_to_rapid(&mut self, p: Point, cmt: &str) {
        self.mv(self.rapid, p.x as f32, p.y as f32, false, cmt);
    }

    /// `Move` with `gcodeCompress` on.
    fn mv(&mut self, gnr: u8, x: f32, y: f32, apply_feed: bool, cmt: &str) {
        if apply_feed && gnr > 0 {
            self.apply_xy_feed = false; // don't set feed next time
        }
        let cmt = if cmt.is_empty() {
            String::new()
        } else {
            format!("({cmt})")
        };
        if gnr > 0 || self.last_x != x || self.last_y != y {
            let mut tmp = String::new();
            let mut needed = false;
            if self.last_g != f32::from(gnr) {
                tmp.push_str(&format!("G{gnr}"));
                needed = true;
            }
            if self.last_x != x {
                tmp.push_str(&format!("X{}", single_format(x, NUMBER_FORMAT)));
                needed = true;
            }
            if self.last_y != y {
                tmp.push_str(&format!("Y{}", single_format(y, NUMBER_FORMAT)));
                needed = true;
            }
            if (gnr == 1 && self.last_f != self.xy_feed) || apply_feed {
                tmp.push_str(&format!("F{}", single_to_string(self.xy_feed)));
                self.last_f = self.xy_feed;
                needed = true;
            }
            // Smoothieware needs the laser power on every G1
            if gnr == 1 && self.last_s != self.spindle_speed && self.firmware == Firmware::Smoothie {
                tmp.push_str(&format!("S{}", single_format(self.spindle_speed, NUMBER_FORMAT)));
                self.last_s = self.spindle_speed;
                needed = true;
            }
            tmp.push_str(&cmt);
            if needed {
                self.out.push(tmp);
            }
        }
        self.last_x = x;
        self.last_y = y;
        self.last_g = f32::from(gnr);
    }

    /// `Arc`: `G2`/`G3` to `end` with the centre at `ij` from the current point, or
    /// short lines when `split` (`avoidG23`).
    #[allow(clippy::cast_possible_truncation)]
    pub fn arc(&mut self, gnr: u8, end: Point, ij: Point, cmt: &str, split: bool) {
        let (x, y, i, j) = (end.x as f32, end.y as f32, ij.x as f32, ij.y as f32);
        let apply = self.apply_xy_feed;
        let feed = if apply {
            self.apply_xy_feed = false;
            format!("F{}", single_to_string(self.xy_feed))
        } else {
            String::new()
        };
        let cmt = if cmt.is_empty() {
            String::new()
        } else {
            format!("({cmt})")
        };
        let spindle = if self.firmware == Firmware::Smoothie {
            format!("S{}", single_format(self.spindle_speed, NUMBER_FORMAT))
        } else {
            String::new()
        };
        if split {
            let (lx, ly) = (self.last_x, self.last_y);
            self.split_arc(gnr, lx, ly, x, y, i, j, &cmt);
        } else {
            self.out.push(format!(
                "G{gnr}X{}Y{}I{}J{}{feed}{spindle}{cmt}",
                single_format(x, NUMBER_FORMAT),
                single_format(y, NUMBER_FORMAT),
                single_format(i, NUMBER_FORMAT),
                single_format(j, NUMBER_FORMAT)
            ));
            self.last_g = f32::from(gnr);
        }
        self.last_x = x;
        self.last_y = y;
        self.last_f = self.xy_feed;
    }

    /// `splitArc`: the arc as lines of about 1 mm chords (`gcodeAngleStep`).
    #[allow(clippy::too_many_arguments, clippy::similar_names)]
    fn split_arc(&mut self, gnr: u8, x1: f32, y1: f32, x2: f32, y2: f32, i: f32, j: f32, cmt: &str) {
        let segment_length = 10.0f32;
        let radius = (i * i + j * j).sqrt();
        let (cx, cy) = (x1 + i, y1 + j);
        let (a1, a2, _) = angles(x1, y1, x2, y2, i, j);
        let mut da = -(360.0 + a1 - a2);
        if gnr == 3 {
            da = (360.0 + a2 - a1).abs();
        }
        if da > 360.0 {
            da -= 360.0;
        }
        if da < -360.0 {
            da += 360.0;
        }
        if x1 == x2 && y1 == y2 {
            da = if gnr == 2 { -360.0 } else { 360.0 };
        }
        #[allow(clippy::cast_possible_truncation)]
        let step = ((1.0f64 / f64::from(radius)).asin() * 180.0 / std::f64::consts::PI) as f32;
        self.apply_xy_feed = true;
        let mut move_length = 10.0f32; // remainingC
        let mut count = 1.0f32;
        let mut cmt = cmt.to_owned();
        let mut emit = |w: &mut Self, angle: f32, cmt: &mut String| {
            let a = std::f64::consts::PI * f64::from(angle) / 180.0;
            #[allow(clippy::cast_possible_truncation)]
            let x = cx + radius * a.cos() as f32;
            #[allow(clippy::cast_possible_truncation)]
            let y = cy + radius * a.sin() as f32;
            move_length += distance(x, y, w.last_x, w.last_y);
            let apply = w.apply_xy_feed;
            let c = cmt.trim_start_matches('(').trim_end_matches(')').to_owned();
            w.mv(1, x, y, apply, &c);
            if move_length >= count * segment_length {
                w.apply_xy_feed = true;
                count += 1.0;
            }
            if cmt.len() > 1 {
                cmt.clear();
            }
        };
        if da > 0.0 {
            let mut angle = a1 + step;
            while angle < a1 + da {
                emit(self, angle, &mut cmt);
                angle += step;
            }
        } else {
            let mut angle = a1 - step;
            while angle > a1 + da {
                emit(self, angle, &mut cmt);
                angle -= step;
            }
        }
        let apply = self.apply_xy_feed;
        self.mv(1, x2, y2, apply, "End Arc conversion");
        if move_length >= count * segment_length {
            self.apply_xy_feed = true;
        }
    }
}

fn distance(x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let (dx, dy) = (x2 - x1, y2 - y1);
    (dx * dx + dy * dy).sqrt()
}

/// `getAngle`: start, end and delta angles (degrees) of an arc.
#[allow(clippy::cast_possible_truncation)]
fn angles(x1: f32, y1: f32, x2: f32, y2: f32, i: f32, j: f32) -> (f32, f32, f32) {
    let radius = (i * i + j * j).sqrt();
    let cos1 = (i / radius).clamp(-1.0, 1.0);
    let mut a1 = 180.0 - 180.0 * ((f64::from(cos1).acos() / std::f64::consts::PI) as f32);
    if j > 0.0 {
        a1 = -a1;
    }
    let cos2 = ((x1 + i - x2) / radius).clamp(-1.0, 1.0);
    let mut a2 = 180.0 - 180.0 * ((f64::from(cos2).acos() / std::f64::consts::PI) as f32);
    if (y1 + j - y2) > 0.0 {
        a2 = -a2;
    }
    let mut da = -(360.0 + a1 - a2);
    if da > 360.0 {
        da -= 360.0;
    }
    if da < -360.0 {
        da += 360.0;
    }
    (a1, a2, da)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine() -> MachineOptions {
        MachineOptions::default()
    }

    #[test]
    fn moves_are_compressed_and_feed_follows_pen_down() {
        let mut w = Writer::new(&machine());
        w.initial();
        w.set_layer_params(2000, 400);
        w.pen_up("fill");
        w.move_to_rapid(Point::new(35.0, 86.0), "fill");
        w.pen_down("fill");
        w.move_to(Point::new(44.999, 86.0), "fill");
        w.move_to(Point::new(44.999, 86.0), "fill");
        w.pen_up("fill");
        w.move_to_rapid(Point::new(44.999, 86.5), "fill");
        w.final_command();
        assert_eq!(
            w.lines(),
            [
                "M3 S0",
                "S0 (fill)",
                "G0X35Y86(fill)",
                "S400 (fill)",
                "G1X44.999F2000(fill)",
                "S0 (fill)",
                "G0Y86.5(fill)",
                "M5 S0"
            ]
        );
    }

    #[test]
    fn without_pwm_the_laser_switches_and_power_is_set_per_layer() {
        let m = MachineOptions {
            support_pwm: false,
            ..machine()
        };
        let mut w = Writer::new(&m);
        w.initial();
        w.set_layer_params(1000, 500);
        w.pen_down("");
        w.pen_up("");
        assert_eq!(w.lines(), ["M5 S1000", "S500", "M3", "M5"]);
    }

    #[test]
    fn smoothie_power_is_scaled_and_repeated_on_g1() {
        let m = MachineOptions {
            firmware: Firmware::Smoothie,
            ..machine()
        };
        let mut w = Writer::new(&m);
        w.set_layer_params(1000, 255);
        w.move_to(Point::new(1.0, 1.0), "");
        w.arc(3, Point::new(2.0, 2.0), Point::new(1.0, 0.0), "", false);
        assert_eq!(w.lines(), ["G1X1Y1F1000S1", "G3X2Y2I1J0S1"]);
    }

    #[test]
    fn arcs_and_split_arcs() {
        let mut w = Writer::new(&machine());
        w.move_to_rapid(Point::new(10.0, 0.0), "");
        w.arc(3, Point::new(10.0, 0.0), Point::new(-10.0, 0.0), "circle", false);
        assert_eq!(w.lines()[1], "G3X10Y0I-10J0F1000(circle)");
        let mut s = Writer::new(&machine());
        s.move_to_rapid(Point::new(10.0, 0.0), "");
        s.arc(3, Point::new(-10.0, 0.0), Point::new(-10.0, 0.0), "half", true);
        let n = s.lines().len();
        assert!(n > 20, "{n}");
        assert!(s.lines()[n - 1].starts_with("X-10Y0"), "{}", s.lines()[n - 1]);
        let mut cw = Writer::new(&machine());
        cw.move_to_rapid(Point::new(10.0, 0.0), "");
        cw.arc(2, Point::new(10.0, 0.0), Point::new(-10.0, 0.0), "", true);
        assert!(cw.lines().len() > 40);
        let mut r = Writer::new(&MachineOptions {
            disable_g0: true,
            ..machine()
        });
        r.move_to_rapid(Point::new(1.0, 2.0), "");
        r.raw("(note)");
        assert_eq!(r.clone().into_lines(), ["G1X1Y2F1000", "(note)"]); // G1 always carries a changed feed
        let (a1, a2, da) = angles(10.0, 0.0, 0.0, 10.0, -10.0, 0.0);
        assert!(
            (a1 - 0.0).abs() < 1e-4 && (a2 - 90.0).abs() < 1e-4 && (da + 270.0).abs() < 1e-3,
            "{a1} {a2} {da}"
        );
    }
}
