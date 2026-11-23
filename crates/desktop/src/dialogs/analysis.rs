//! Analysis settings (Q75, Q76): input boxes for each analysis instead of
//! SPICE directives, values with SI suffixes, validated while typing. The
//! form logic lives in [`crate::analysis_form`]. The dialog is a modal
//! window of its own over the main window (D22).

use crate::analysis_form::{
    AnalysisForm, Field, FieldErrors, METHODS, OPTION_FIELDS, SCALES, SOLVERS, label_of,
    text_fields,
};
use crate::simulation::summary::{ANALYSES, analysis_label};
use crate::ui::{form_row, hint, pressable, section_title, testable};
use crate::workspace::Workspace;
use gpui_kit::component::{
    Disableable, IconName, Selectable, Sizable,
    button::{Button, ButtonGroup, ButtonVariants},
    checkbox::Checkbox,
    dialog::DialogFooter,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{prelude::*, *};
use std::collections::BTreeMap;
use xsim_editor::dialog_window::{self, Build, DialogContent, DialogOptions};
use xsim_project::{AnalysisChoice, SimulationSetup};
use xsim_spice_engine::IntegrationMethod;

/// Under "Start from rest" when it is off (D5, D16): initial conditions
/// still apply and constrain the operating point.
pub(crate) const OPERATING_POINT_START: &str = "The run starts from the DC operating point, as \
     in SPICE. A capacitor or inductor with an initial condition starts at it, and the rest of \
     the circuit is solved around it.";

/// Under "Start from rest" when it is on (D5).
pub(crate) const REST_START: &str = "Sources switch on at 0 s. Capacitors start discharged and \
     inductors de-energized, or at their initial conditions.";

/// Shown while Backward Euler is the integration method: the engine keeps
/// the tolerances, so the first-order method takes many more steps.
pub(crate) const BACKWARD_EULER_NOTE: &str = "Backward Euler needs far more time points than \
     trapezoidal at the default tolerances, because precision comes first, so runs can be slow.";

/// The hint under "Start from rest" for its state.
fn start_explanation(start_from_rest: bool) -> &'static str {
    if start_from_rest {
        REST_START
    } else {
        OPERATING_POINT_START
    }
}

/// Debug selector of [`BACKWARD_EULER_NOTE`].
const BACKWARD_EULER_SELECTOR: &str = "backward-euler-note";

/// Element id and debug selector of the analysis-kind button for `choice`.
fn kind_button_selector(choice: AnalysisChoice) -> &'static str {
    match choice {
        AnalysisChoice::OperatingPoint => "analysis-kind-op",
        AnalysisChoice::DcSweep => "analysis-kind-dc",
        AnalysisChoice::Ac => "analysis-kind-ac",
        AnalysisChoice::Transient => "analysis-kind-tran",
    }
}

/// What the dialog edits.
#[derive(Clone)]
pub struct AnalysisDialogInput {
    /// The active circuit's settings.
    pub setup: SimulationSetup,
    /// Names of the schematic's independent sources, for the DC sweep.
    pub sources: Vec<String>,
    /// Whether the project is read-only (settings are shown, not applied).
    pub read_only: bool,
}

/// Title of the dialog and its window.
const TITLE: &str = "Analysis settings";

/// Opens the dialog over `window`; Apply, or Enter in the form, stores the
/// settings in `workspace`.
pub fn open_analysis_dialog(
    workspace: Entity<Workspace>,
    input: AnalysisDialogInput,
    window: &mut Window,
    cx: &mut App,
) {
    let read_only = input.read_only;
    let reserved = reserved_states(&input);
    let options = DialogOptions::new(TITLE, 36.).reserve_states(reserved.len());
    // Built for the hidden window that measures the form, for each reserved
    // state, then for the dialog's window (D24). None of it reaches the
    // workspace until Apply.
    dialog_window::open(window, cx, options, move |build, window, cx| {
        let form = cx.new(|cx| AnalysisDialog::new(input.clone(), window, cx));
        match build {
            // In the form rather than on the dialog itself, so that Enter
            // applies what was chosen with the mouse.
            Build::Shown => form.focus_handle(cx).focus(window, cx),
            Build::Reserved(state) => {
                form.update(cx, |form, cx| form.enter(reserved[state], cx));
            }
        }
        let workspace = workspace.clone();
        move |content: DialogContent, _: &mut Window, _: &mut App| {
            let (on_ok_form, on_ok_workspace) = (form.clone(), workspace.clone());
            content
                .title(TITLE)
                .child(form.clone())
                .on_ok(move |_, cx| confirm(&on_ok_form, &on_ok_workspace, read_only, cx))
                .footer(footer(&form, &workspace, read_only))
        }
    });
}

/// A state the form can change to while the dialog is open: an analysis
/// type with the solver options open and the notes they can show. The
/// window keeps room on the screen for every one (D24), so it opens where
/// the tallest fits below its top, nearer the top of the screen than
/// centred if need be. Error messages are not reserved: with them the form
/// may scroll above the buttons.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Reserved {
    choice: AnalysisChoice,
    /// "Start from rest", whose explanation is a line longer or shorter;
    /// `None` keeps it as it is.
    start_from_rest: Option<bool>,
    /// Whether Backward Euler is chosen, which shows its note.
    backward_euler: bool,
}

/// The states the form for `input` can reach: in a read-only project only
/// the analysis type and the solver options' toggle change.
fn reserved_states(input: &AnalysisDialogInput) -> Vec<Reserved> {
    let editable = !input.read_only;
    let backward_euler = editable || input.setup.options.method == IntegrationMethod::BackwardEuler;
    ANALYSES
        .into_iter()
        .flat_map(|choice| {
            let starts: &[Option<bool>] = if editable && choice == AnalysisChoice::Transient {
                &[Some(false), Some(true)]
            } else {
                &[None]
            };
            starts.iter().map(move |&start_from_rest| Reserved {
                choice,
                start_from_rest,
                backward_euler,
            })
        })
        .collect()
}

/// Enter that no focused control took; whether the dialog closes. It
/// applies like the Apply button and keeps the dialog open on errors; a
/// read-only project has nothing to apply.
fn confirm(
    form: &Entity<AnalysisDialog>,
    workspace: &Entity<Workspace>,
    read_only: bool,
    cx: &mut App,
) -> bool {
    !read_only && apply(form, workspace, cx)
}

/// Validates the form and stores it in `workspace`; whether it was valid.
fn apply(form: &Entity<AnalysisDialog>, workspace: &Entity<Workspace>, cx: &mut App) -> bool {
    match form.update(cx, |form, cx| form.apply(cx)) {
        Some(setup) => {
            workspace.update(cx, |this, cx| this.apply_simulation_setup(setup, cx));
            true
        }
        None => false,
    }
}

/// Cancel and Apply.
fn footer(
    form: &Entity<AnalysisDialog>,
    workspace: &Entity<Workspace>,
    read_only: bool,
) -> DialogFooter {
    let (form, workspace) = (form.clone(), workspace.clone());
    DialogFooter::new()
        .child(testable(
            "analysis-cancel",
            pressable(Button::new("analysis-cancel"))
                .label("Cancel")
                .on_click(|_, window, cx| dialog_window::close(window, cx)),
        ))
        .child(testable(
            "analysis-apply",
            pressable(Button::new("analysis-apply"))
                .primary()
                .label("Apply")
                .disabled(read_only)
                .on_click(move |_, window, cx| {
                    if apply(&form, &workspace, cx) {
                        dialog_window::close(window, cx);
                    }
                }),
        ))
}

/// The dialog's content view.
pub struct AnalysisDialog {
    form: AnalysisForm,
    base: SimulationSetup,
    /// Independent sources of the schematic, for the DC sweep.
    sources: Vec<String>,
    inputs: BTreeMap<Field, Entity<InputState>>,
    errors: FieldErrors,
    read_only: bool,
    show_advanced: bool,
    /// Holds the focus while no control in the form has it.
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl AnalysisDialog {
    fn new(input: AnalysisDialogInput, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let AnalysisDialogInput {
            setup: base,
            sources,
            read_only,
        } = input;
        let mut form = AnalysisForm::from_setup(&base);
        if !sources.contains(&form.dc_source)
            && let Some(first) = sources.first()
        {
            form.dc_source = first.clone();
        }
        let mut inputs = BTreeMap::new();
        let mut subscriptions = Vec::new();
        for field in crate::analysis_form::ALL_TEXT_FIELDS {
            let text = form.text(field).to_string();
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(text)
                    .placeholder(field.placeholder())
            });
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this, input, event, _, cx| {
                    if let InputEvent::Change = event {
                        let text = input.read(cx).value().to_string();
                        this.text_changed(field, text, cx);
                    }
                },
            ));
            inputs.insert(field, input);
        }
        Self {
            form,
            base,
            sources,
            inputs,
            errors: FieldErrors::new(),
            read_only,
            show_advanced: false,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// Inline validation of one box as the user types.
    fn text_changed(&mut self, field: Field, text: String, cx: &mut Context<Self>) {
        self.form.set_text(field, text);
        match self.form.field_error(field) {
            Some(message) => self.errors.insert(field, message),
            None => self.errors.remove(&field),
        };
        cx.notify();
    }

    /// Validates everything; the new setup, or `None` with the errors
    /// shown.
    fn apply(&mut self, cx: &mut Context<Self>) -> Option<SimulationSetup> {
        let validated = self.form.validate(&self.base, &self.sources);
        cx.notify();
        match validated {
            Ok(setup) => {
                self.errors.clear();
                Some(setup)
            }
            Err(errors) => {
                if errors.keys().any(|f| OPTION_FIELDS.contains(f)) {
                    self.show_advanced = true;
                }
                self.errors = errors;
                None
            }
        }
    }

    fn select(&mut self, choice: AnalysisChoice, cx: &mut Context<Self>) {
        self.form.selected = choice;
        cx.notify();
    }

    /// Puts the form in `state`, with the solver options open. Only in the
    /// hidden window that measures the dialog's content (D24).
    fn enter(&mut self, state: Reserved, cx: &mut Context<Self>) {
        self.form.selected = state.choice;
        if let Some(start_from_rest) = state.start_from_rest {
            self.form.start_from_rest = start_from_rest;
        }
        if state.backward_euler {
            self.form.method = IntegrationMethod::BackwardEuler;
        }
        self.show_advanced = true;
        cx.notify();
    }

    /// A text box row with its unit and message.
    fn text_row(&self, field: Field, cx: &App) -> Div {
        let input = Input::new(&self.inputs[&field])
            .small()
            .disabled(self.read_only);
        form_row(
            field.label(),
            input,
            field.unit(),
            self.errors.get(&field).map(String::as_str),
            cx,
        )
    }

    fn text_rows(&self, fields: &[Field], cx: &App) -> Vec<Div> {
        fields
            .iter()
            .map(|&field| self.text_row(field, cx))
            .collect()
    }

    /// Operating point / DC sweep / AC sweep / Transient, in the design of
    /// the source-type selector of the part properties (D14). Each button
    /// handles its own click, because the group's callback never sees
    /// Enter or Space on a focused button, and Enter presses the focused
    /// button rather than applying the dialog. A read-only project can
    /// still switch, to view the other analyses' settings.
    fn kind_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.form.selected;
        let buttons = ANALYSES.map(|choice| {
            pressable(Button::new(kind_button_selector(choice)))
                .label(analysis_label(choice))
                .selected(choice == current)
                .debug_selector(move || kind_button_selector(choice).into())
                .on_click(cx.listener(move |this, _, _, cx| this.select(choice, cx)))
        });
        ButtonGroup::new("analysis-kind")
            .small()
            .outline()
            .children(buttons)
    }

    /// The boxes of the selected analysis.
    fn analysis_section(&self, cx: &mut Context<Self>) -> Div {
        let section = v_flex().gap_3();
        let fields = text_fields(self.form.selected);
        match self.form.selected {
            AnalysisChoice::OperatingPoint => section.child(hint(
                "Solves the DC bias point: capacitors open, inductors shorted. \
                 Every node voltage and branch current is recorded.",
                cx,
            )),
            AnalysisChoice::DcSweep => section
                .child(self.source_row(cx))
                .children(self.text_rows(fields, cx)),
            AnalysisChoice::Ac => section
                .child(form_row("Sweep", self.scale_choice(cx), "", None, cx))
                .children(self.text_rows(fields, cx))
                .child(hint(
                    "Small-signal response around the operating point; each source's AC magnitude drives the circuit.",
                    cx,
                )),
            AnalysisChoice::Transient => section
                .children(self.text_rows(fields, cx))
                .child(self.start_from_rest(cx)),
        }
    }

    /// The source swept by a DC sweep.
    fn source_row(&self, cx: &mut Context<Self>) -> Div {
        let error = self.errors.get(&Field::DcSource).map(String::as_str);
        if self.sources.is_empty() {
            return form_row(
                "Source",
                hint("The schematic has no voltage or current source.", cx),
                "",
                error,
                cx,
            );
        }
        let this = cx.weak_entity();
        let (current, sources) = (self.form.dc_source.clone(), self.sources.clone());
        let menu = Button::new("dc-source")
            .small()
            .outline()
            .label(current.clone())
            .dropdown_caret(true)
            .disabled(self.read_only)
            .debug_selector(|| "dc-source".into())
            // A schematic may have more sources than the dialog's window
            // (D22) is high: the menu scrolls.
            .dropdown_menu(move |menu, _, _| {
                sources.iter().fold(menu.scrollable(true), |menu, source| {
                    let (this, source) = (this.clone(), source.clone());
                    let (name, selector) = (source.clone(), format!("dc-source-{source}"));
                    menu.item(
                        PopupMenuItem::element(move |_, _| {
                            let selector = selector.clone();
                            div().debug_selector(move || selector).child(name.clone())
                        })
                        .checked(source == current)
                        .on_click(move |_, _, cx| {
                            this.update(cx, |this, cx| {
                                this.form.dc_source = source.clone();
                                this.errors.remove(&Field::DcSource);
                                cx.notify();
                            })
                            .ok();
                        }),
                    )
                })
            });
        form_row("Source", menu, "", error, cx)
    }

    /// Decade / octave / linear spacing.
    fn scale_choice(&self, cx: &mut Context<Self>) -> AnyElement {
        self.choice(
            "ac-scale",
            self.form.ac_scale,
            &SCALES,
            cx,
            |form, scale| form.ac_scale = scale,
        )
    }

    /// "Start from rest" (D5) and how initial conditions apply (D16).
    fn start_from_rest(&self, cx: &mut Context<Self>) -> Div {
        let explanation = start_explanation(self.form.start_from_rest);
        v_flex()
            .gap_1()
            .pl(rems(9.75))
            .child(testable(
                "start-from-rest",
                Checkbox::new("start-from-rest")
                    .label("Start from rest")
                    .checked(self.form.start_from_rest)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        if !this.read_only {
                            this.form.start_from_rest = *checked;
                            cx.notify();
                        }
                    })),
            ))
            .child(hint(explanation, cx))
    }

    /// The muted note shown while Backward Euler is chosen, or nothing.
    fn backward_euler_note(&self, cx: &App) -> Option<Div> {
        (self.form.method == IntegrationMethod::BackwardEuler).then(|| {
            hint(BACKWARD_EULER_NOTE, cx).debug_selector(|| BACKWARD_EULER_SELECTOR.into())
        })
    }

    /// The collapsible solver options. The Backward Euler note shows under
    /// the integration method, or under the toggle while the options are
    /// collapsed, so a slow run is explained without opening them.
    fn advanced_section(&self, cx: &mut Context<Self>) -> Div {
        let toggle = pressable(Button::new("advanced-toggle"))
            .small()
            .ghost()
            .debug_selector(|| "advanced-toggle".into())
            .icon(if self.show_advanced {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .label("Advanced solver options")
            .on_click(cx.listener(|this, _, _, cx| {
                this.show_advanced = !this.show_advanced;
                cx.notify();
            }));
        let section = v_flex().gap_3().child(div().child(toggle));
        if !self.show_advanced {
            return section.children(self.backward_euler_note(cx));
        }
        let method = self.choice("method", self.form.method, &METHODS, cx, |form, method| {
            form.method = method
        });
        let solver = self.choice("solver", self.form.solver, &SOLVERS, cx, |form, solver| {
            form.solver = solver
        });
        // Under the menu, closer to it than to the next row.
        let note = self
            .backward_euler_note(cx)
            .map(|note| div().pl(rems(9.75)).mt(rems(-0.5)).child(note));
        section
            .child(form_row("Integration", method, "", None, cx))
            .children(note)
            .child(form_row("Linear solver", solver, "", None, cx))
            .children(self.text_rows(&OPTION_FIELDS, cx))
    }

    /// A drop-down over a `(value, label)` table that writes the choice into
    /// the form with `set`.
    fn choice<T: Copy + PartialEq + 'static>(
        &self,
        id: &'static str,
        current: T,
        table: &'static [(T, &'static str)],
        cx: &mut Context<Self>,
        set: fn(&mut AnalysisForm, T),
    ) -> AnyElement {
        let this = cx.weak_entity();
        Button::new(id)
            .small()
            .outline()
            .debug_selector(move || id.to_string())
            .label(label_of(table, current))
            .dropdown_caret(true)
            .disabled(self.read_only)
            .dropdown_menu(move |menu, _, _| {
                table.iter().fold(menu, |menu, &(value, label)| {
                    let this = this.clone();
                    menu.item(
                        PopupMenuItem::new(label)
                            .checked(value == current)
                            .on_click(move |_, _, cx| {
                                this.update(cx, |this, cx| {
                                    set(&mut this.form, value);
                                    cx.notify();
                                })
                                .ok();
                            }),
                    )
                })
            })
            .into_any_element()
    }
}

impl Focusable for AnalysisDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for AnalysisDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .track_focus(&self.focus)
            .gap_4()
            .child(self.kind_selector(cx))
            .child(self.analysis_section(cx))
            .child(section_title("Solver", cx))
            .child(self.advanced_section(cx))
            .when(self.read_only, |this| {
                this.child(hint(
                    "This project is read-only; the settings can be viewed but not changed.",
                    cx,
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AnalysisDialog, AnalysisDialogInput, BACKWARD_EULER_SELECTOR, OPERATING_POINT_START,
        REST_START, kind_button_selector, start_explanation,
    };
    use crate::simulation::summary::ANALYSES;
    use crate::test_support::{click, init, press};
    use gpui_kit::component::Theme;
    use gpui_kit::{
        AppContext, Bounds, Entity, Pixels, Point, ScrollDelta, ScrollEvent, TestAppContext,
        VisualContext as _, VisualTestContext, WindowBounds, WindowOptions, point, px, size,
    };
    use xsim_editor::dialog_window;
    use xsim_project::{AnalysisChoice, SimulationSetup};
    use xsim_spice_engine::IntegrationMethod;

    /// The dialog's content in a window of its own.
    fn dialog(
        cx: &mut TestAppContext,
        read_only: bool,
    ) -> (Entity<AnalysisDialog>, VisualTestContext) {
        init(cx);
        let input = AnalysisDialogInput {
            setup: SimulationSetup::default(),
            sources: vec!["V1".into()],
            read_only,
        };
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Point::default(),
                        size(px(700.), px(600.)),
                    ))),
                    ..WindowOptions::default()
                },
                cx,
                |window, cx| cx.new(|cx| AnalysisDialog::new(input, window, cx)),
            )
            .expect("open the dialog window")
        });
        let visual = VisualTestContext::from_window(window, cx);
        visual.run_until_parked();
        (view, visual)
    }

    fn selected(view: &Entity<AnalysisDialog>, cx: &VisualTestContext) -> AnalysisChoice {
        view.read_with(cx, |dialog, _| dialog.form.selected)
    }

    fn kind_bounds(cx: &mut VisualTestContext) -> Vec<Bounds<Pixels>> {
        ANALYSES
            .iter()
            .map(|&choice| {
                cx.debug_bounds(kind_button_selector(choice))
                    .unwrap_or_else(|| panic!("{choice:?} button is not shown"))
            })
            .collect()
    }

    /// D14: one row of joined small buttons, as the source-type selector.
    #[gpui_kit::test]
    fn the_kinds_are_a_joined_row_of_small_buttons(cx: &mut TestAppContext) {
        let (_, mut cx) = dialog(cx, false);
        let rem = cx.update(|window, _| window.rem_size());
        let bounds = kind_bounds(&mut cx);
        for pair in bounds.windows(2) {
            assert_eq!(pair[0].top(), pair[1].top(), "one row");
            assert_eq!(pair[0].right(), pair[1].left(), "joined, no gap");
        }
        for button in &bounds {
            // A small kit button is 1.5 rem high.
            assert_eq!(button.size.height, rem * 1.5);
        }
    }

    #[gpui_kit::test]
    fn clicking_a_kind_selects_it(cx: &mut TestAppContext) {
        let (view, mut cx) = dialog(cx, false);
        click("analysis-kind-ac", &mut cx);
        assert_eq!(selected(&view, &cx), AnalysisChoice::Ac);
        click("analysis-kind-op", &mut cx);
        assert_eq!(selected(&view, &cx), AnalysisChoice::OperatingPoint);
    }

    /// The buttons are tab stops; Enter and Space choose the focused one.
    #[gpui_kit::test]
    fn the_kinds_work_from_the_keyboard(cx: &mut TestAppContext) {
        let (view, mut cx) = dialog(cx, false);
        // Tab order follows the row: operating point, DC sweep, AC sweep.
        for _ in 0..3 {
            cx.update(|window, cx| window.focus_next(cx));
        }
        cx.run_until_parked();
        press("enter", &mut cx);
        assert_eq!(selected(&view, &cx), AnalysisChoice::Ac);
        cx.update(|window, cx| window.focus_prev(cx));
        cx.run_until_parked();
        press("space", &mut cx);
        assert_eq!(selected(&view, &cx), AnalysisChoice::DcSweep);
    }

    /// A read-only project can still look at every analysis.
    #[gpui_kit::test]
    fn read_only_settings_can_still_be_browsed(cx: &mut TestAppContext) {
        let (view, mut cx) = dialog(cx, true);
        click("analysis-kind-dc", &mut cx);
        assert_eq!(selected(&view, &cx), AnalysisChoice::DcSweep);
    }
    /// D16: with the default operating-point start, initial conditions
    /// still apply and constrain the operating point; both hints speak of
    /// initial conditions the same way.
    #[test]
    fn the_start_hints_explain_initial_conditions() {
        assert_eq!(start_explanation(false), OPERATING_POINT_START);
        assert_eq!(start_explanation(true), REST_START);
        assert!(OPERATING_POINT_START.contains("DC operating point"));
        assert!(OPERATING_POINT_START.contains("initial condition starts at it"));
        assert!(OPERATING_POINT_START.contains("solved around it"));
        assert!(REST_START.contains("or at their initial conditions"));
        for hint in [OPERATING_POINT_START, REST_START] {
            assert!(!hint.contains("  "), "{hint:?}");
        }
    }

    fn set_method(
        view: &Entity<AnalysisDialog>,
        method: IntegrationMethod,
        advanced: bool,
        cx: &mut VisualTestContext,
    ) {
        view.update(cx, |dialog, cx| {
            dialog.form.method = method;
            dialog.show_advanced = advanced;
            cx.notify();
        });
        cx.run_until_parked();
    }

    /// The Backward Euler note shows only for that method, with the solver
    /// options open (under the method) or closed (under the toggle).
    #[gpui_kit::test]
    fn backward_euler_shows_a_note(cx: &mut TestAppContext) {
        let (view, mut cx) = dialog(cx, false);
        assert!(cx.debug_bounds(BACKWARD_EULER_SELECTOR).is_none());
        for advanced in [false, true] {
            set_method(&view, IntegrationMethod::BackwardEuler, advanced, &mut cx);
            assert!(
                cx.debug_bounds(BACKWARD_EULER_SELECTOR).is_some(),
                "advanced {advanced}"
            );
        }
        let note = cx
            .debug_bounds(BACKWARD_EULER_SELECTOR)
            .expect("the note is shown");
        let integration = cx.debug_bounds("method").expect("the method menu");
        assert!(note.top() >= integration.bottom(), "under the method menu");
        assert!(
            (note.left() - integration.left()).abs() <= gpui_kit::px(1.),
            "aligned with the controls: {note:?}, {integration:?}"
        );
        for method in [
            IntegrationMethod::Trapezoidal,
            IntegrationMethod::ModifiedTrapezoidal,
            IntegrationMethod::Gear2,
        ] {
            for advanced in [false, true] {
                set_method(&view, method, advanced, &mut cx);
                assert!(
                    cx.debug_bounds(BACKWARD_EULER_SELECTOR).is_none(),
                    "{method:?}, advanced {advanced}"
                );
            }
        }
    }

    /// Backward Euler chosen in the real Integration menu shows the note
    /// under the menu, and it stays under the toggle once the options are
    /// folded away; choosing trapezoidal again removes it.
    #[gpui_kit::test]
    fn choosing_backward_euler_in_the_menu_shows_the_note(cx: &mut TestAppContext) {
        let (view, mut cx) = dialog(cx, false);
        let method = |view: &Entity<AnalysisDialog>, cx: &VisualTestContext| {
            view.read_with(cx, |dialog, _| dialog.form.method)
        };
        click("advanced-toggle", &mut cx);
        assert!(cx.debug_bounds(BACKWARD_EULER_SELECTOR).is_none());
        click("method", &mut cx);
        press("up", &mut cx);
        press("enter", &mut cx);
        assert_eq!(method(&view, &cx), IntegrationMethod::BackwardEuler);
        let note = cx
            .debug_bounds(BACKWARD_EULER_SELECTOR)
            .expect("the note is shown");
        let menu = cx.debug_bounds("method").expect("the method menu");
        assert!(note.top() >= menu.bottom(), "{note:?} under {menu:?}");
        click("advanced-toggle", &mut cx);
        assert!(cx.debug_bounds("method").is_none(), "folded away");
        let toggle = cx.debug_bounds("advanced-toggle").expect("the toggle");
        let note = cx
            .debug_bounds(BACKWARD_EULER_SELECTOR)
            .expect("the note stays while folded");
        assert!(note.top() >= toggle.bottom(), "{note:?} under {toggle:?}");
        click("advanced-toggle", &mut cx);
        click("method", &mut cx);
        press("down", &mut cx);
        press("enter", &mut cx);
        assert_eq!(method(&view, &cx), IntegrationMethod::Trapezoidal);
        assert!(cx.debug_bounds(BACKWARD_EULER_SELECTOR).is_none());
    }

    /// The cases a dialog may open with: each analysis, with "Start from
    /// rest" on or off, the Backward Euler note, without a source to sweep
    /// and read-only.
    fn opening_cases() -> Vec<AnalysisDialogInput> {
        use xsim_spice_engine::InitialState;
        let mut cases = Vec::new();
        for choice in ANALYSES {
            for rest in [false, true] {
                for method in [
                    IntegrationMethod::Trapezoidal,
                    IntegrationMethod::BackwardEuler,
                ] {
                    for read_only in [false, true] {
                        for sources in [true, false] {
                            let relevant = (!rest || choice == AnalysisChoice::Transient)
                                && (sources || choice == AnalysisChoice::DcSweep);
                            if !relevant {
                                continue;
                            }
                            let mut setup = SimulationSetup {
                                selected: choice,
                                ..SimulationSetup::default()
                            };
                            setup.options.method = method;
                            if rest {
                                setup.transient.initial_state = InitialState::ZeroState;
                            }
                            cases.push(AnalysisDialogInput {
                                setup,
                                sources: if sources {
                                    vec!["V1".into()]
                                } else {
                                    Vec::new()
                                },
                                read_only,
                            });
                        }
                    }
                }
            }
        }
        cases
    }

    fn describe(input: &AnalysisDialogInput) -> String {
        let setup = &input.setup;
        format!(
            "{:?}, {:?}, {:?}, read-only {}, {} sources",
            setup.selected,
            setup.transient.initial_state,
            setup.options.method,
            input.read_only,
            input.sources.len()
        )
    }

    /// Opens the dialog for `input` over the main window `window`.
    fn open_dialog(
        workspace: &Entity<crate::workspace::Workspace>,
        input: AnalysisDialogInput,
        window: &mut VisualTestContext,
    ) -> VisualTestContext {
        let workspace = workspace.clone();
        window.update(|window, cx| super::open_analysis_dialog(workspace, input, window, cx));
        window.run_until_parked();
        crate::test_support::dialog(window)
    }

    /// Checks the dialog in `settings` against D24: inside the work area,
    /// showing all of its form without scrolling unless that is taller than
    /// the work area, and Cancel and Apply inside the window.
    fn assert_fully_shown(case: &str, settings: &mut VisualTestContext) {
        let handle = settings.window_handle();
        let layout = settings
            .cx
            .update(|app| dialog_window::layout_of(handle, app))
            .unwrap_or_else(|| panic!("{case}: no dialog that fits its content"));
        if let Err(problem) = layout.check() {
            panic!("{case}: {problem} ({layout:?})");
        }
        // Apart from the dialog's own measurements: a form that fits is not
        // cut off, but between the title row and the footer.
        if layout.natural_height <= layout.room.size.height {
            let content = settings.debug_bounds("dialog-content").expect("the form");
            let bar = settings
                .debug_bounds("dialog-title-bar")
                .expect("the title");
            let footer = settings.debug_bounds("dialog-footer").expect("the footer");
            assert!(
                content.top() >= bar.bottom() - px(1.) && content.bottom() <= footer.top() + px(1.),
                "{case}: the form {content:?} is cut off between {bar:?} and {footer:?}"
            );
        }
        let viewport = layout.viewport;
        for button in ["analysis-cancel", "analysis-apply"] {
            let bounds = settings
                .debug_bounds(button)
                .unwrap_or_else(|| panic!("{case}: {button} is not shown"));
            assert!(
                bounds.top() >= px(0.)
                    && bounds.bottom() <= viewport.height + px(1.)
                    && bounds.right() <= viewport.width + px(1.),
                "{case}: {button} {bounds:?} outside {viewport:?}"
            );
        }
    }

    /// Clicks `selector` in the dialog, scrolling its form until it is in
    /// view: on a small screen at a large UI scale the form is taller than
    /// the window and scrolls above the buttons (D24).
    fn click_in_view(selector: &'static str, settings: &mut VisualTestContext) {
        let bounds = settings
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is not shown"));
        let footer = settings.debug_bounds("dialog-footer").expect("the footer");
        let top = settings
            .debug_bounds("dialog-title-bar")
            .map_or(px(0.), |bar| bar.bottom());
        if bounds.top() < top || bounds.bottom() > footer.top() {
            let middle = (top + footer.top()) / 2.;
            settings.simulate_event(ScrollEvent {
                position: point(footer.center().x, middle),
                delta: ScrollDelta::Pixels(point(px(0.), middle - bounds.center().y)),
                ..ScrollEvent::default()
            });
            settings.run_until_parked();
        }
        click(selector, settings);
    }

    /// D24: the window opens at its measured height whatever it shows,
    /// fully shown, centred across the main window and down as far as its
    /// room to grow allows: never lower than centred. Opened again, it
    /// opens the same way.
    #[gpui_kit::test]
    fn the_window_opens_where_it_fits_whatever_it_shows(cx: &mut TestAppContext) {
        use crate::test_support::{TestEnv, open, press_in_dialog, try_dialog};
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        let owner = window.update(|window, _| window.bounds());
        let cases = opening_cases();
        let mut first = Vec::new();
        for (pass, input) in cases.iter().chain(cases.iter().take(2)).enumerate() {
            let case = describe(input);
            let mut settings = open_dialog(&workspace, input.clone(), &mut window);
            assert_fully_shown(&case, &mut settings);
            let bounds = settings.update(|window, _| window.bounds());
            assert!(
                (bounds.center().x - owner.center().x).abs() <= px(1.),
                "{case}: centred across {bounds:?} over {owner:?}"
            );
            let centred = owner.center().y - bounds.size.height / 2.;
            assert!(
                bounds.top() <= centred + px(1.),
                "{case}: below centre {bounds:?} over {owner:?}"
            );
            if pass < cases.len() {
                first.push(bounds);
            } else {
                assert_eq!(bounds, first[pass - cases.len()], "{case}: again");
            }
            press_in_dialog("escape", &mut settings);
            assert!(try_dialog(&window).is_none(), "{case}: closed");
        }
    }

    /// D24: on the owner's screen at 200% display scaling (a 1920 × 1030 px
    /// work area) and on a smaller one, at 100% and 150% UI scale, the
    /// window opens fully shown, Cancel and Apply without scrolling, and
    /// stays so through everything the form can show while open: each
    /// analysis, "Start from rest" on and off, the solver options open and
    /// folded, and the Backward Euler note. Read-only too.
    #[gpui_kit::test]
    fn every_state_of_the_form_stays_fully_shown_on_the_owners_screens(cx: &mut TestAppContext) {
        use crate::test_support::{TestEnv, open, press_in_dialog};
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        for (width, height) in [(1920., 1030.), (1280., 680.)] {
            let area = Bounds::new(Point::default(), size(px(width), px(height)));
            for font_size in [16., 24.] {
                window.update(|window, cx| {
                    dialog_window::simulate_work_area(cx, Some(area));
                    Theme::update(cx, |theme| theme.font_size = px(font_size));
                    window.resize(area.size);
                    window.bounds_changed(cx);
                });
                window.run_until_parked();
                for read_only in [false, true] {
                    let screen = format!("{font_size} px on {width} × {height}");
                    let input = AnalysisDialogInput {
                        setup: SimulationSetup::default(),
                        sources: vec!["V1".into()],
                        read_only,
                    };
                    let mut settings = open_dialog(&workspace, input, &mut window);
                    let step = |what: &str, settings: &mut VisualTestContext| {
                        let case = format!("{screen}, read-only {read_only}: {what}");
                        assert_fully_shown(&case, settings);
                    };
                    step("opened", &mut settings);
                    for choice in ANALYSES {
                        click_in_view(kind_button_selector(choice), &mut settings);
                        step(&format!("{choice:?}"), &mut settings);
                    }
                    if !read_only {
                        for _ in 0..2 {
                            click_in_view("start-from-rest", &mut settings);
                            step("start from rest toggled", &mut settings);
                        }
                    }
                    click_in_view("advanced-toggle", &mut settings);
                    step("solver options open", &mut settings);
                    if !read_only {
                        click_in_view("method", &mut settings);
                        press("up", &mut settings);
                        press("enter", &mut settings);
                        assert!(
                            settings.debug_bounds(BACKWARD_EULER_SELECTOR).is_some(),
                            "{screen}: Backward Euler chosen"
                        );
                        step("Backward Euler", &mut settings);
                    }
                    for choice in ANALYSES {
                        click_in_view(kind_button_selector(choice), &mut settings);
                        step(&format!("{choice:?} with the options open"), &mut settings);
                    }
                    click_in_view("advanced-toggle", &mut settings);
                    step("solver options folded", &mut settings);
                    press_in_dialog("escape", &mut settings);
                }
            }
        }
    }
}
