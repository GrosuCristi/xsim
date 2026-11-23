//! # xsim waveform post-processing
//!
//! UI-independent computations behind the waveform viewer:
//! * [`expr`] — trace expressions such as `V(out)-V(in)` or `V(out)*I(R1)` (Q132);
//! * [`measure`] — average, RMS, peak, rise/fall/settling time, frequency (Q135);
//! * [`lod`] — min/max level-of-detail envelopes for fast drawing (Q140);
//! * [`ticks`] — "nice" linear and logarithmic axis ticks;
//! * [`export`] — CSV export (Q137).
//!
//! Everything works on plain sample slices or on an engine
//! [`SimulationResult`](xsim_spice_engine::SimulationResult), and nothing
//! panics on malformed input.
//!
//! ```
//! use xsim_waveform::{Expression, measure, ticks};
//!
//! let power = Expression::parse("V(out) * I(R1)")?;
//! assert_eq!(power.signals(), ["I(R1)", "V(out)"]);
//! assert_eq!(power.unit(), "W");
//!
//! // A 0 → 2 → 0 triangle over 2 s: mean 1, RMS 2/√3.
//! let stats = measure::stats(&[0.0, 1.0, 2.0], &[0.0, 2.0, 0.0], None).unwrap();
//! assert_eq!(stats.average, 1.0);
//! assert!((stats.rms - 2.0 / 3f64.sqrt()).abs() < 1e-15);
//!
//! assert_eq!(ticks::linear(0.0, 1.0, 5), [0.0, 0.2, 0.4, 0.6, 0.8, 1.0]);
//! # Ok::<(), xsim_waveform::ExprError>(())
//! ```

pub mod export;
pub mod expr;
pub mod lod;
pub mod measure;
pub mod ticks;

pub use expr::{ExprError, Expression, Series};
