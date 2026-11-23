//! Analysis settings (Q75, Q76): input boxes for each analysis instead of
//! SPICE directives, values with SI suffixes, validated while typing. The
//! form logic lives in [`crate::analysis_form`].

use crate::analysis_form::{
    AnalysisForm, Field, FieldErrors, METHODS, OPTION_FIELDS, SCALES, SOLVERS, label_of,
    text_fields,
};
use crate::simulation::summary::{ANALYSES, analysis_label};
use crate::ui::{dialog_width, form_row, hint, pressable, section_title, testable};
use crate::workspace::Workspace;
use gpui_kit::component::{
    Disableable, IconName, Selectable, Sizable, WindowExt,
    button::{Button, ButtonGroup, ButtonVariants},
    checkbox::Checkbox,
    dialog::DialogFooter,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{prelude::*, *};
use std::collections::BTreeMap;
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
pub struct AnalysisDialogInput {
    /// The active circuit's settings.
    pub setup: SimulationSetup,
    /// Names of the schematic's independent sources, for the DC sweep.
    pub sources: Vec<String>,
    /// Whether the project is read-only (settings are shown, not applied).
    pub read_only: bool,
}

/// Opens the dialog; Apply, or Enter in the form, stores the settings in
/// `workspace`.
pub fn open_analysis_dialog(
    workspace: Entity<Workspace>,
    input: AnalysisDialogInput,
    window: &mut Window,
    cx: &mut App,
) {
    let read_only = input.read_only;
    let form = cx.new(|cx| AnalysisDialog::new(input, window, cx));
    window.open_dialog(cx, {
        let form = form.clone();
        move |dialog, window, _| {
            let (on_ok_form, on_ok_workspace) = (form.clone(), workspace.clone());
            dialog
                .title("Analysis settings")
                .w(dialog_width(36., window))
                .child(form.clone())
                .on_ok(move |_, window, cx| {
                    confirm(&on_ok_form, &on_ok_workspace, read_only, window, cx)
                })
                .footer(footer(&form, &workspace, read_only))
        }
    });
    // In the form rather than on the dialog itself, so that Enter applies
    // what was chosen with the mouse.
    form.focus_handle(cx).focus(window, cx);
}

/// Enter that no focused control took, as the kit confirms a dialog with
/// it; whether the dialog closes. In the form (a box, the checkbox or
/// nothing in particular) it applies like the Apply button and keeps the
/// dialog open on errors; a read-only project has nothing to apply. On the
/// kit's close button it closes, as that button does.
fn confirm(
    form: &Entity<AnalysisDialog>,
    workspace: &Entity<Workspace>,
    read_only: bool,
    window: &Window,
    cx: &mut App,
) -> bool {
    if !form.focus_handle(cx).contains_focused(window, cx) {
        return true;
    }
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
                .on_click(|_, window, cx| window.close_dialog(cx)),
        ))
        .child(testable(
            "analysis-apply",
            pressable(Button::new("analysis-apply"))
                .primary()
                .label("Apply")
                .disabled(read_only)
                .on_click(move |_, window, cx| {
                    if apply(&form, &workspace, cx) {
                        window.close_dialog(cx);
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
            .dropdown_menu(move |menu, _, _| {
                sources.iter().fold(menu, |menu, source| {
                    let (this, source) = (this.clone(), source.clone());
                    menu.item(
                        PopupMenuItem::new(source.clone())
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
            .child(
                Checkbox::new("start-from-rest")
                    .label("Start from rest")
                    .checked(self.form.start_from_rest)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        if !this.read_only {
                            this.form.start_from_rest = *checked;
                            cx.notify();
                        }
                    })),
            )
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
    use gpui_kit::{
        AppContext, Bounds, Entity, Pixels, Point, TestAppContext, VisualTestContext, WindowBounds,
        WindowOptions, px, size,
    };
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
}
