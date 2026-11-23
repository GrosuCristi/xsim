//! Level-of-detail vertices per trace, cached across frames (Q140).
//!
//! Hover and cursor drags repaint the plot on every mouse move; decimating
//! millions of samples each time would lag, so the decimated vertices are
//! kept until the trace, the visible x range or the plot width changes.

use std::sync::Arc;
use xsim_waveform::lod::{Vertex, decimate};

/// How many decimations are kept (a few traces in a few panes).
const CAPACITY: usize = 64;

/// What a decimation depends on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LodKey {
    /// `PlotValues::id` of the trace.
    pub trace: u64,
    pub x_min: f64,
    pub x_max: f64,
    pub columns: usize,
    pub log_x: bool,
}

/// Most-recently-used decimations.
#[derive(Debug, Default)]
pub struct LodCache {
    entries: Vec<(LodKey, Arc<Vec<Vertex>>)>,
}

impl LodCache {
    /// The vertices for `key`, decimating `x`/`y` on a miss.
    pub fn vertices(&mut self, key: LodKey, x: &[f64], y: &[f64]) -> Arc<Vec<Vertex>> {
        if let Some(index) = self.entries.iter().position(|(k, _)| *k == key) {
            let entry = self.entries.remove(index);
            let vertices = entry.1.clone();
            self.entries.push(entry);
            return vertices;
        }
        let vertices = Arc::new(decimate(
            x,
            y,
            key.x_min,
            key.x_max,
            key.columns.max(1),
            key.log_x,
        ));
        if self.entries.len() >= CAPACITY {
            self.entries.remove(0);
        }
        self.entries.push((key, vertices.clone()));
        vertices
    }

    /// Number of cached decimations.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is cached.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
