//! Vector artwork of every symbol, in symbol-local grid units.
//!
//! The leads end exactly at the pins of [`xsim_project::symbols::geometry`],
//! so the drawing and the electrical connections always agree. One list of
//! shapes serves the canvas, the toolbar icons and the library preview.
//!
//! The artwork is designed in schema-2 units, twice the grid unit (D22), and
//! [`shapes`] scales it by [`ART_SCALE`]: symbols keep their size on screen,
//! and since the factor is a power of two the leads still end exactly on
//! the pins.

use crate::model::geometry::{Rect, Vec2};
use std::f32::consts::PI;
use xsim_project::{Orientation, PartKind};

/// One drawing primitive.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// An open polyline stroked with the symbol pen.
    Line(Vec<Vec2>),
    /// A circle outline.
    Circle { center: Vec2, radius: f32 },
    /// A filled polygon (arrowheads, the diode triangle).
    Fill(Vec<Vec2>),
    /// A `+` or `−` sign. Signs stay upright on screen whatever the
    /// orientation, so they always read correctly.
    Sign { at: Vec2, plus: bool },
}

/// Grid units per unit of the artwork's design coordinates (schema-2
/// units, D22).
const ART_SCALE: f32 = 2.0;

/// Half the length of a `+` or `−` sign's bar, in grid units.
pub const SIGN_ARM: f32 = 0.2 * ART_SCALE;

const fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

/// The artwork of `kind`, in grid units.
pub fn shapes(kind: PartKind) -> Vec<Shape> {
    designed(kind)
        .into_iter()
        .map(|shape| shape.scaled(ART_SCALE))
        .collect()
}

impl Shape {
    /// The shape with every coordinate and radius multiplied by `factor`.
    fn scaled(self, factor: f32) -> Shape {
        let scale = |points: Vec<Vec2>| points.into_iter().map(|p| p * factor).collect();
        match self {
            Shape::Line(points) => Shape::Line(scale(points)),
            Shape::Fill(points) => Shape::Fill(scale(points)),
            Shape::Circle { center, radius } => Shape::Circle {
                center: center * factor,
                radius: radius * factor,
            },
            Shape::Sign { at, plus } => Shape::Sign {
                at: at * factor,
                plus,
            },
        }
    }
}

/// The artwork of `kind` in its design coordinates.
fn designed(kind: PartKind) -> Vec<Shape> {
    match kind {
        PartKind::Resistor => resistor(),
        PartKind::Capacitor => capacitor(),
        PartKind::Inductor => inductor(),
        PartKind::VoltageSource => voltage_source(),
        PartKind::CurrentSource => current_source(),
        PartKind::Diode => diode(),
        PartKind::NpnBjt => bjt(true),
        PartKind::PnpBjt => bjt(false),
        PartKind::NJfet => jfet(true),
        PartKind::PJfet => jfet(false),
        PartKind::VoltageSwitch => voltage_switch(),
        PartKind::CurrentSwitch => current_switch(),
        PartKind::Ground => ground(),
        PartKind::OpAmp => opamp(),
    }
}

/// The op-amp (D26): a triangle pointing right, its output lead leaving the
/// apex. The inputs enter the left side, the inverting one on top, each
/// marked inside the triangle (`−` beside `−`, `+` beside `+`); the supply
/// leads V+ (top) and V− (bottom) run vertically onto the slanted sides.
/// Design coordinates: `+ (−2, 1)`, `− (−2, −1)`, `OUT (2, 0)`,
/// `V+ (0, −2)`, `V− (0, 2)`; the sides cross x = 0 at y = ∓0.75.
fn opamp() -> Vec<Shape> {
    vec![
        Shape::Line(vec![
            v(-1.5, -1.5),
            v(1.5, 0.0),
            v(-1.5, 1.5),
            v(-1.5, -1.5),
        ]),
        Shape::Line(vec![v(-2.0, 1.0), v(-1.5, 1.0)]),
        Shape::Line(vec![v(-2.0, -1.0), v(-1.5, -1.0)]),
        Shape::Line(vec![v(2.0, 0.0), v(1.5, 0.0)]),
        Shape::Line(vec![v(0.0, -2.0), v(0.0, -0.75)]),
        Shape::Line(vec![v(0.0, 2.0), v(0.0, 0.75)]),
        Shape::Sign {
            at: v(-1.1, 1.0),
            plus: true,
        },
        Shape::Sign {
            at: v(-1.1, -1.0),
            plus: false,
        },
    ]
}

/// The point of the segment `a`–`b` at height `y` (the segment must not be
/// horizontal).
fn at_height(a: Vec2, b: Vec2, y: f32) -> Vec2 {
    a + (b - a) * ((y - a.y) / (b.y - a.y))
}

/// The point of the segment `a`–`b` at abscissa `x` (the segment must not
/// be vertical).
fn at_abscissa(a: Vec2, b: Vec2, x: f32) -> Vec2 {
    a + (b - a) * ((x - a.x) / (b.x - a.x))
}

/// A dashed line from `from` to `to`, starting and ending with a dash: the
/// mechanical link between a switch's control and its blade.
fn dashed(from: Vec2, to: Vec2) -> Vec<Shape> {
    const DASH: f32 = 0.22;
    const GAP: f32 = 0.16;
    let length = (to - from).length();
    if length <= DASH {
        return vec![Shape::Line(vec![from, to])];
    }
    // Whole dashes and gaps, stretched to fit exactly.
    let dashes = ((length + GAP) / (DASH + GAP)).round().max(1.0);
    let scale = length / (dashes * DASH + (dashes - 1.0) * GAP);
    let direction = (to - from) * (1.0 / length);
    (0..dashes as usize)
        .map(|i| {
            let start = i as f32 * (DASH + GAP) * scale;
            Shape::Line(vec![
                from + direction * start,
                from + direction * (start + DASH * scale),
            ])
        })
        .collect()
}

/// The switch path runs down the right side like a transistor's collector
/// and emitter: P's lead ends in the fixed contact, and the blade is hinged
/// on N's lead, drawn open (the state a switch starts in). On the left the
/// control box sits between the C+ and C− leads, marked `+` and `−` beside
/// them, and drives the blade through a dashed link.
fn voltage_switch() -> Vec<Shape> {
    let hinge = v(2.0, 0.9);
    let tip = v(1.25, -0.85);
    let (left, right, top, bottom) = (-1.3, -0.3, -1.6, 1.6);
    let mut shapes = vec![
        Shape::Line(vec![v(2.0, -2.0), v(2.0, -0.9)]),
        Shape::Line(vec![v(2.0, 2.0), hinge, tip]),
        Shape::Line(vec![v(-2.0, -1.0), v(left, -1.0)]),
        Shape::Line(vec![v(-2.0, 1.0), v(left, 1.0)]),
        Shape::Line(vec![
            v(left, top),
            v(right, top),
            v(right, bottom),
            v(left, bottom),
            v(left, top),
        ]),
        Shape::Sign {
            at: v(-0.8, -1.0),
            plus: true,
        },
        Shape::Sign {
            at: v(-0.8, 1.0),
            plus: false,
        },
    ];
    shapes.extend(dashed(v(right, 0.0), at_height(hinge, tip, 0.0)));
    shapes
}

/// The blade is hinged on P's lead and drawn open above N's fixed contact.
/// A dashed link drops from it to a current arrow, the mark of a switch
/// driven by a current; the part's text names the source that current
/// flows through.
fn current_switch() -> Vec<Shape> {
    let hinge = v(-0.9, 0.0);
    let tip = v(0.75, -0.7);
    let arrow_y = 0.75;
    let (tail, head) = (v(-0.5, arrow_y), v(0.55, arrow_y));
    let mut shapes = vec![
        Shape::Line(vec![v(-2.0, 0.0), hinge, tip]),
        Shape::Line(vec![v(2.0, 0.0), v(0.9, 0.0)]),
        Shape::Line(vec![tail, head - v(0.3, 0.0)]),
        arrow(tail, head, 1.0, true),
    ];
    let link = at_abscissa(hinge, tip, 0.0);
    shapes.extend(dashed(link, v(0.0, arrow_y - 0.2)));
    shapes
}

fn resistor() -> Vec<Shape> {
    vec![Shape::Line(vec![
        v(-2.0, 0.0),
        v(-1.2, 0.0),
        v(-1.0, -0.4),
        v(-0.6, 0.4),
        v(-0.2, -0.4),
        v(0.2, 0.4),
        v(0.6, -0.4),
        v(1.0, 0.4),
        v(1.2, 0.0),
        v(2.0, 0.0),
    ])]
}

fn capacitor() -> Vec<Shape> {
    vec![
        Shape::Line(vec![v(-2.0, 0.0), v(-0.25, 0.0)]),
        Shape::Line(vec![v(-0.25, -0.75), v(-0.25, 0.75)]),
        Shape::Line(vec![v(0.25, -0.75), v(0.25, 0.75)]),
        Shape::Line(vec![v(0.25, 0.0), v(2.0, 0.0)]),
    ]
}

fn inductor() -> Vec<Shape> {
    const BUMPS: usize = 4;
    const RADIUS: f32 = 0.3;
    const STEPS: usize = 16;
    let mut points = vec![v(-2.0, 0.0), v(-1.2, 0.0)];
    for bump in 0..BUMPS {
        let cx = -1.2 + RADIUS + 2.0 * RADIUS * bump as f32;
        for step in 1..=STEPS {
            let angle = PI * (1.0 - step as f32 / STEPS as f32);
            points.push(v(cx + RADIUS * angle.cos(), -RADIUS * angle.sin()));
        }
    }
    points.push(v(2.0, 0.0));
    vec![Shape::Line(points)]
}

fn source_body() -> Vec<Shape> {
    vec![
        Shape::Line(vec![v(-2.0, 0.0), v(-0.9, 0.0)]),
        Shape::Line(vec![v(0.9, 0.0), v(2.0, 0.0)]),
        Shape::Circle {
            center: v(0.0, 0.0),
            radius: 0.9,
        },
    ]
}

fn voltage_source() -> Vec<Shape> {
    let mut shapes = source_body();
    shapes.push(Shape::Sign {
        at: v(-0.45, 0.0),
        plus: true,
    });
    shapes.push(Shape::Sign {
        at: v(0.45, 0.0),
        plus: false,
    });
    shapes
}

/// Current flows into the + pin, through the source, out of the − pin.
fn current_source() -> Vec<Shape> {
    let mut shapes = source_body();
    shapes.push(Shape::Line(vec![v(-0.5, 0.0), v(0.25, 0.0)]));
    shapes.push(Shape::Fill(vec![v(0.55, 0.0), v(0.2, -0.24), v(0.2, 0.24)]));
    shapes
}

fn diode() -> Vec<Shape> {
    vec![
        Shape::Line(vec![v(-2.0, 0.0), v(-0.5, 0.0)]),
        Shape::Fill(vec![v(-0.5, -0.55), v(0.45, 0.0), v(-0.5, 0.55)]),
        Shape::Line(vec![v(0.45, -0.55), v(0.45, 0.55)]),
        Shape::Line(vec![v(0.45, 0.0), v(2.0, 0.0)]),
    ]
}

/// A filled arrowhead on the segment `from`–`to`, its tip at fraction `t`,
/// pointing towards `to` (`forward`) or back towards `from`.
fn arrow(from: Vec2, to: Vec2, t: f32, forward: bool) -> Shape {
    const LENGTH: f32 = 0.42;
    const HALF_WIDTH: f32 = 0.2;
    let d = to - from;
    let u = d * (1.0 / d.length());
    let dir = if forward { u } else { u * -1.0 };
    let tip = from + d * t;
    let base = tip - dir * LENGTH;
    let n = Vec2::new(-u.y, u.x) * HALF_WIDTH;
    Shape::Fill(vec![tip, base + n, base - n])
}

fn bjt(npn: bool) -> Vec<Shape> {
    let emitter_start = v(0.3, 0.35);
    let emitter_bend = v(2.0, 1.2);
    vec![
        Shape::Circle {
            center: v(1.0, 0.0),
            radius: 1.45,
        },
        Shape::Line(vec![v(-2.0, 0.0), v(0.3, 0.0)]),
        Shape::Line(vec![v(0.3, -0.8), v(0.3, 0.8)]),
        Shape::Line(vec![v(0.3, -0.35), v(2.0, -1.2), v(2.0, -2.0)]),
        Shape::Line(vec![emitter_start, emitter_bend, v(2.0, 2.0)]),
        if npn {
            arrow(emitter_start, emitter_bend, 0.85, true)
        } else {
            arrow(emitter_start, emitter_bend, 0.35, false)
        },
    ]
}

fn jfet(n_channel: bool) -> Vec<Shape> {
    let gate_start = v(-2.0, 0.0);
    let gate_end = v(0.4, 0.0);
    vec![
        Shape::Circle {
            center: v(1.0, 0.0),
            radius: 1.45,
        },
        Shape::Line(vec![gate_start, gate_end]),
        Shape::Line(vec![v(0.4, -0.9), v(0.4, 0.9)]),
        Shape::Line(vec![v(0.4, -0.6), v(2.0, -0.6), v(2.0, -2.0)]),
        Shape::Line(vec![v(0.4, 0.6), v(2.0, 0.6), v(2.0, 2.0)]),
        if n_channel {
            arrow(gate_start, gate_end, 0.92, true)
        } else {
            arrow(gate_start, gate_end, 0.72, false)
        },
    ]
}

fn ground() -> Vec<Shape> {
    vec![
        Shape::Line(vec![v(0.0, 0.0), v(0.0, 0.45)]),
        Shape::Line(vec![v(-0.8, 0.45), v(0.8, 0.45)]),
        Shape::Line(vec![v(-0.5, 0.7), v(0.5, 0.7)]),
        Shape::Line(vec![v(-0.2, 0.95), v(0.2, 0.95)]),
    ]
}

/// Maps a symbol-local point through `orientation` (mirror, then rotate),
/// exactly like [`Orientation::apply`] but for fractional points.
pub fn oriented(orientation: Orientation, local: Vec2) -> Vec2 {
    let mut p = local;
    if orientation.mirror_x {
        p.x = -p.x;
    }
    if orientation.mirror_y {
        p.y = -p.y;
    }
    for _ in 0..(orientation.rotation % 4) {
        p = Vec2::new(-p.y, p.x);
    }
    p
}

/// Local extent of the artwork.
pub fn extent(shapes: &[Shape]) -> Option<Rect> {
    let points = shapes.iter().flat_map(|s| match s {
        Shape::Line(points) | Shape::Fill(points) => points.clone(),
        Shape::Circle { center, radius } => vec![
            *center - Vec2::new(*radius, *radius),
            *center + Vec2::new(*radius, *radius),
        ],
        Shape::Sign { at, .. } => vec![*at],
    });
    Rect::enclosing(points)
}

#[cfg(test)]
mod tests {
    use super::{Shape, extent, oriented, shapes};
    use crate::model::geometry::{Rect, Vec2};
    use xsim_project::{Orientation, PartKind, Point, symbols};

    #[test]
    fn every_pin_is_the_end_of_a_lead() {
        for kind in PartKind::ALL {
            let art = shapes(kind);
            for &pin in symbols::geometry(kind).pins {
                let target = Vec2::from_grid(pin);
                let touches = art.iter().any(|s| match s {
                    Shape::Line(points) => {
                        points.first() == Some(&target) || points.last() == Some(&target)
                    }
                    _ => false,
                });
                assert!(touches, "{kind:?} has no lead ending at pin {pin:?}");
            }
        }
        for kind in [PartKind::VoltageSwitch, PartKind::CurrentSwitch] {
            assert!(PartKind::ALL.contains(&kind), "{kind:?} is checked above");
        }
    }

    /// D17: each switch pin has exactly one lead, which starts on the pin
    /// and runs into the body; the artwork stays inside the body box that
    /// halos, hit tests and text placement use; the voltage switch marks
    /// C+ with `+` and C− with `−`, and the current switch carries its
    /// current arrow.
    #[test]
    fn switch_symbols_fit_their_body_and_mark_their_control() {
        for kind in [PartKind::VoltageSwitch, PartKind::CurrentSwitch] {
            let g = symbols::geometry(kind);
            let art = shapes(kind);
            let body = Rect::from_corners(Vec2::from_grid(g.body_min), Vec2::from_grid(g.body_max));
            let drawn = extent(&art).expect("the symbol has artwork");
            assert!(
                body.inflate(1e-4, 1e-4).contains_rect(drawn),
                "{kind:?}: {drawn:?} leaves the body {body:?}"
            );
            let centre = body.center();
            for &pin in g.pins {
                let target = Vec2::from_grid(pin);
                let leads: Vec<&Vec<Vec2>> = art
                    .iter()
                    .filter_map(|s| match s {
                        Shape::Line(points) if points.first() == Some(&target) => Some(points),
                        _ => None,
                    })
                    .collect();
                assert_eq!(leads.len(), 1, "{kind:?} pin {pin:?}");
                assert!(
                    leads[0][1].distance(centre) < target.distance(centre),
                    "{kind:?}: the lead of pin {pin:?} points away from the body"
                );
            }
        }
        let pins = symbols::geometry(PartKind::VoltageSwitch).pins;
        let (plus_pin, minus_pin) = (Vec2::from_grid(pins[2]), Vec2::from_grid(pins[3]));
        let signs: Vec<(Vec2, bool)> = shapes(PartKind::VoltageSwitch)
            .into_iter()
            .filter_map(|s| match s {
                Shape::Sign { at, plus } => Some((at, plus)),
                _ => None,
            })
            .collect();
        assert_eq!(signs.iter().filter(|(_, plus)| *plus).count(), 1);
        assert_eq!(signs.len(), 2, "{signs:?}");
        for (at, plus) in signs {
            let (own, other) = if plus {
                (plus_pin, minus_pin)
            } else {
                (minus_pin, plus_pin)
            };
            assert!(at.distance(own) < at.distance(other), "{at:?} {plus}");
        }
        assert!(
            shapes(PartKind::CurrentSwitch)
                .iter()
                .any(|s| matches!(s, Shape::Fill(_))),
            "the current switch has its arrow"
        );
    }

    /// D26: the op-amp's art stays inside its body box; each of its five
    /// pins has exactly one lead, starting on the pin and running into the
    /// body; the supply leads end on the triangle's sides; the inputs are
    /// marked `+` beside the `+` pin and `−` beside the `−` pin; and the
    /// output lead leaves the apex.
    #[test]
    fn the_opamp_symbol_fits_its_body_and_marks_its_inputs() {
        let kind = PartKind::OpAmp;
        let g = symbols::geometry(kind);
        let art = shapes(kind);
        let body = Rect::from_corners(Vec2::from_grid(g.body_min), Vec2::from_grid(g.body_max));
        let drawn = extent(&art).expect("the symbol has artwork");
        assert!(
            body.inflate(1e-4, 1e-4).contains_rect(drawn),
            "{drawn:?} leaves the body {body:?}"
        );
        let centre = body.center();
        let mut inner_ends = Vec::new();
        for &pin in g.pins {
            let target = Vec2::from_grid(pin);
            let leads: Vec<&Vec<Vec2>> = art
                .iter()
                .filter_map(|s| match s {
                    Shape::Line(points) if points.first() == Some(&target) => Some(points),
                    _ => None,
                })
                .collect();
            assert_eq!(leads.len(), 1, "pin {pin:?}");
            assert_eq!(leads[0].len(), 2, "pin {pin:?}: a straight lead");
            assert!(
                leads[0][1].distance(centre) < target.distance(centre),
                "the lead of pin {pin:?} points away from the body"
            );
            inner_ends.push(leads[0][1]);
        }
        // The triangle: apex at the output lead's inner end, and the
        // supply leads end on its slanted sides.
        let triangle = art
            .iter()
            .find_map(|s| match s {
                Shape::Line(points) if points.len() == 4 => Some(points.clone()),
                _ => None,
            })
            .expect("a closed triangle");
        assert_eq!(triangle.first(), triangle.last(), "closed");
        assert!(
            triangle.contains(&inner_ends[2]),
            "the output leaves the apex"
        );
        let apex = inner_ends[2];
        let back = triangle[0].x;
        for supply in [inner_ends[3], inner_ends[4]] {
            let corner = triangle
                .iter()
                .copied()
                .find(|p| p.x == back && (p.y < 0.0) == (supply.y < 0.0))
                .expect("a back corner on the supply's side");
            let along = (supply.x - corner.x) / (apex.x - corner.x);
            let expected = corner + (apex - corner) * along;
            assert!(
                supply.distance(expected) < 1e-5,
                "the supply lead ends on the side: {supply:?} vs {expected:?}"
            );
        }
        for input in [inner_ends[0], inner_ends[1]] {
            assert!((input.x - back).abs() < 1e-5, "inputs enter the back side");
        }
        let signs: Vec<(Vec2, bool)> = art
            .iter()
            .filter_map(|s| match s {
                Shape::Sign { at, plus } => Some((*at, *plus)),
                _ => None,
            })
            .collect();
        assert_eq!(signs.len(), 2, "{signs:?}");
        let (plus_pin, minus_pin) = (Vec2::from_grid(g.pins[0]), Vec2::from_grid(g.pins[1]));
        assert_eq!((g.pin_names[0], g.pin_names[1]), ("+", "−"));
        for (at, plus) in signs {
            let (own, other) = if plus {
                (plus_pin, minus_pin)
            } else {
                (minus_pin, plus_pin)
            };
            assert!(at.distance(own) < at.distance(other), "{at:?} {plus}");
            assert!(at.x > back, "the marks sit inside the triangle");
        }
    }

    #[test]
    fn fractional_orientation_matches_the_grid_transform() {
        let local = Point::new(2, -1);
        for rotation in 0..4 {
            for (mirror_x, mirror_y) in [(false, false), (true, false), (false, true), (true, true)]
            {
                let o = Orientation {
                    rotation,
                    mirror_x,
                    mirror_y,
                };
                assert_eq!(
                    oriented(o, Vec2::from_grid(local)),
                    Vec2::from_grid(o.apply(local))
                );
            }
        }
    }
}
