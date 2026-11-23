# Waveform viewer and measurements

[Question index](QUESTION_INDEX.md)

## Plot rendering, panes, and probing

### Q127

★ Should the waveform viewer be a specialized custom plotting implementation, or should GPUI Kit's built-in chart infrastructure be extended?

extend the built in gpui chart infra

### Q128

Must users be able to create multiple independent plot panes and synchronized waveform windows?

yes

### Q129

Should plots support linear, logarithmic, decibel, phase, and XY/scatter axes?

yes

### Q130

Must a click on a schematic wire or component immediately create the appropriate trace in the waveform viewer?

yes

## Cursors, traces, and analysis views

### Q131

What cursor capabilities are required: two cursors, delta measurements, snapping, tracking, and crosshair synchronization?

yes, all of them

### Q132

Must users create calculated traces using expressions like `V(out)-V(in)` and `V(out)*I(R1)`?

yes, of course

### Q133

How should parameter sweeps appear: separate traces, color groups, filters, overlays, and comparison tables?

all of them

### Q134

Should the viewer have dedicated Bode, FFT, frequency-response, and harmonic-analysis views?

yes

### Q135

Should measurements be interactive, with average, RMS, peak, rise time, settling time, and frequency calculations?

yes

### Q136

Must plot styling and configurations be reusable between simulation runs and projects?

not between projects but between simulations runs yes

## File formats, streaming, and large datasets

### Q137

Which import/export formats are required: CSV, TSV, JSON, image formats, LTspice `.raw`, and `.plt`?

csv, and image formats like svg png and jpg

### Q138

Must existing LTspice waveform files be openable directly, including different RAW encodings?

NO

### Q139

Should traces update live while simulations run, or only after completion?

no, we can show a loader in with a percentage complete

### Q140

How many waveform samples should remain responsive, and should we use disk-backed datasets plus level-of-detail rendering?

yes, use disk backed datasets. the samples should be responsive enough to be able to visualize the data without any significant lag or data loss

## Related questions

- [Q28](UI_SPECIFICATION.md#q28) — GPUI rendering APIs
- [Q38](PROJECT_FILE_FORMAT.md#q38) — Project storage of results and plots
- [Q62](SCHEMATIC_EDITOR.md#q62) — Schematic probing
- [Q125](SPICE_ENGINE.md#q125) — Recorded simulation quantities
