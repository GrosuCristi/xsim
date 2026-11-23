//! The properties dialog's view: owns one text input per form field, keeps
//! the [`PartForm`] in sync with them and applies the edit to the editor.
//! The sections it renders are built in [`super::sections`].

use super::form::{FieldStatus, PartForm};
use super::spec::FieldId;
use super::waveform::WaveKind;
use crate::SchematicEditor;
use crate::library_browser::{self, LibraryFilter, LibraryItem, ModelSummary};
use gpui_kit::component::{
    input::{InputEvent, InputState},
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Styled as _, Subscription, Window,
};
use std::collections::{HashMap, HashSet};
use xsim_project::{Component, Id};

/// A text input and its change subscription.
struct FieldInput {
    state: Entity<InputState>,
    _changes: Subscription,
}

/// Content of the properties dialog for one component.
pub(crate) struct PropertiesView {
    pub(super) form: PartForm,
    inputs: HashMap<FieldId, FieldInput>,
    editor: Entity<SchematicEditor>,
    component: Id,
    pub(super) read_only: bool,
    /// Why the last apply was refused by the editor.
    pub(super) apply_error: Option<String>,
    /// Library metadata of the chosen model; `None` when it is unknown.
    pub(super) model: Option<ModelSummary>,
    /// Holds the focus while no control in the form has it (after a click
    /// on a blank spot, or once a focused PWL row is removed), so Enter
    /// still applies the form.
    focus: FocusHandle,
    /// Tracks the controls of the selected waveform type (its fields or the
    /// PWL table), which a change of type replaces.
    pub(super) wave_body: FocusHandle,
}

impl PropertiesView {
    /// The form for `component`, whose designator must differ from `taken`;
    /// a current switch's control is chosen from `sources`.
    pub(crate) fn new(
        editor: Entity<SchematicEditor>,
        component: &Component,
        taken: HashSet<String>,
        sources: Vec<String>,
        read_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self {
            form: PartForm::new(&component.name, &component.part, taken)
                .with_control_sources(sources),
            inputs: HashMap::new(),
            editor,
            component: component.id,
            read_only,
            apply_error: None,
            model: None,
            focus: cx.focus_handle(),
            wave_body: cx.focus_handle(),
        };
        view.sync_inputs(window, cx);
        view.refresh_model();
        view
    }

    /// Dialog title: the applied designator and the part kind, e.g.
    /// "R1 · Resistor".
    pub(crate) fn title(&self) -> String {
        format!(
            "{} \u{b7} {}",
            self.form.applied_name(),
            self.form.kind().label()
        )
    }

    /// The input of field `id`, if the field is shown.
    pub(super) fn input(&self, id: FieldId) -> Option<&Entity<InputState>> {
        self.inputs.get(&id).map(|input| &input.state)
    }

    /// The field that takes the keyboard when the dialog opens: the main
    /// value, else the designator.
    pub(crate) fn initial_focus(&self) -> Option<Entity<InputState>> {
        let id = self
            .form
            .numeric_fields()
            .first()
            .map_or(FieldId::Designator, |spec| spec.id);
        self.input(id).cloned()
    }

    /// Creates inputs for fields that appeared and drops those of removed
    /// PWL rows. Inputs of hidden waveform types are kept with their text.
    fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholders: HashMap<FieldId, &'static str> = self
            .form
            .numeric_fields()
            .iter()
            .map(|spec| (spec.id, spec.empty.unwrap_or_default()))
            .collect();
        for id in self.form.text_fields() {
            if !self.inputs.contains_key(&id) {
                let placeholder = placeholders.get(&id).copied().unwrap_or_default();
                let input = self.create_input(id, placeholder, window, cx);
                self.inputs.insert(id, input);
            }
        }
        let rows: HashSet<u64> = self.form.pwl_rows().iter().copied().collect();
        self.inputs.retain(|id, _| match id {
            FieldId::PwlTime(row) | FieldId::PwlValue(row) => rows.contains(row),
            _ => true,
        });
    }

    fn create_input(
        &self,
        id: FieldId,
        placeholder: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> FieldInput {
        let text = self.form.text(id).to_string();
        let state = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(text)
                .placeholder(placeholder)
        });
        let changes = cx.subscribe(&state, move |this, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.text_changed(id, state.read(cx).value().to_string(), cx);
            }
        });
        FieldInput {
            state,
            _changes: changes,
        }
    }

    fn text_changed(&mut self, id: FieldId, text: String, cx: &mut Context<Self>) {
        self.form.set_text(id, text);
        if id == FieldId::Designator {
            self.apply_error = None;
        }
        cx.notify();
    }

    /// Status of the designator, including a refusal by the editor.
    pub(super) fn designator_status(&self) -> FieldStatus {
        match &self.apply_error {
            Some(message) => FieldStatus::Invalid(message.clone()),
            None => self.form.designator_status(),
        }
    }

    pub(super) fn set_wave_kind(
        &mut self,
        kind: WaveKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A click on a type button takes no focus, so a field or button of
        // the type being left may still have the keyboard. Once hidden it
        // would hold it outside the dialog, where Enter and Escape do
        // nothing: the form takes it instead.
        let hides_focus =
            kind != self.form.wave_kind() && self.wave_body.contains_focused(window, cx);
        self.form.set_wave_kind(kind);
        self.sync_inputs(window, cx);
        if hides_focus {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    /// Appends a PWL point and focuses its time.
    pub(super) fn add_pwl_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let row = self.form.add_pwl_row();
        self.sync_inputs(window, cx);
        if let Some(input) = self.input(FieldId::PwlTime(row)) {
            input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        }
        cx.notify();
    }

    /// Removes a PWL point. The form keeps the focus, which may have been on
    /// the removed row.
    pub(super) fn remove_pwl_row(&mut self, row: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.form.remove_pwl_row(row);
        self.sync_inputs(window, cx);
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn set_pwl_repeat(&mut self, repeat: bool, cx: &mut Context<Self>) {
        self.form.set_pwl_repeat(repeat);
        cx.notify();
    }

    /// Chooses the voltage source whose current controls a current switch.
    pub(super) fn set_control(&mut self, control: String, cx: &mut Context<Self>) {
        self.form.set_control(control);
        cx.notify();
    }

    /// Opens the library on the part's model family; picking a model sets it.
    pub(super) fn choose_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(filter) = LibraryFilter::for_kind(self.form.kind()) else {
            return;
        };
        let current = self.form.model_item();
        let view = cx.entity().downgrade();
        library_browser::open_library_at(
            filter,
            current,
            move |item, _, cx| {
                // The dialog may have closed meanwhile; then there is nothing to update.
                let _ = view.update(cx, |view, cx| view.set_model(&item, cx));
            },
            window,
            cx,
        );
    }

    /// Sets the model picked from the library.
    pub(super) fn set_model(&mut self, item: &LibraryItem, cx: &mut Context<Self>) {
        if let LibraryItem::Diode { model }
        | LibraryItem::Bjt { model, .. }
        | LibraryItem::Jfet { model, .. } = item
        {
            self.form.set_model(model.clone());
            self.refresh_model();
            cx.notify();
        }
    }

    fn refresh_model(&mut self) {
        self.model = self
            .form
            .model_item()
            .and_then(|item| library_browser::model_summary(&item));
    }

    /// Writes the form to the schematic as one undoable edit. Returns whether
    /// the dialog may close: nothing to apply, applied, or read-only. On
    /// invalid input or a refusal the first problem field takes the keyboard.
    pub(crate) fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.read_only {
            return true;
        }
        let edit = match self.form.pending_edit() {
            Ok(None) => return true,
            Ok(Some(edit)) => edit,
            Err(errors) => {
                self.focus_field(errors[0].0, window, cx);
                cx.notify();
                return false;
            }
        };
        let (name, part) = edit;
        let result = self.editor.update(cx, |editor, cx| {
            editor.apply_component_edit(self.component, name.clone(), part.clone(), cx)
        });
        match result {
            Ok(()) => {
                self.form.mark_applied(name, part);
                self.apply_error = None;
            }
            Err(message) => {
                self.apply_error = Some(message);
                self.focus_field(FieldId::Designator, window, cx);
            }
        }
        cx.notify();
        self.apply_error.is_none()
    }

    fn focus_field(&self, id: FieldId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = self.input(id) {
            input.update(cx, |input, cx| input.focus(window, cx));
        }
    }
}

impl Focusable for PropertiesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PropertiesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("properties-form")
            .track_focus(&self.focus)
            .w_full()
            .gap_4()
            .pb_1()
            .child(self.general_section(cx))
            .children(self.value_section(cx))
            .children(self.control_section(cx))
            .children(self.switch_section(cx))
            .children(self.source_sections(cx))
            .children(self.model_section(cx))
            .children(self.ground_note(cx))
    }
}
