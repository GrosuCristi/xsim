//! Circuit tabs (Q23, Q35): switching, adding, renaming and deleting.

use super::Workspace;
use super::circuit_tab::CircuitTab;
use crate::ids::TabId;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{prelude::*, *};
use xsim_project::CircuitDocument;

/// An inline rename in progress.
pub(super) struct Rename {
    pub tab: TabId,
    pub input: Entity<InputState>,
    _subscription: Subscription,
}

impl Workspace {
    /// Shows circuit `index`; everything of the other circuits stays as it
    /// was (Q23).
    pub(super) fn switch_to(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        // Leaving the rename box for another circuit applies the typed name,
        // as moving the focus anywhere else does.
        if self.renaming.is_some() && self.renaming_index() != Some(index) {
            self.commit_rename(false, window, cx);
        }
        self.doc.project.active_circuit = index;
        self.focus_editor(window, cx);
        cx.notify();
    }

    /// Adds "Circuit N" and shows it.
    pub(super) fn add_circuit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.doc.read_only {
            return;
        }
        let doc = CircuitDocument::new(self.doc.project.next_circuit_name());
        let tab = CircuitTab::new(self.tab_ids.allocate(), &doc, false, cx);
        self.doc.project.circuits.push(doc);
        self.tabs.push(tab);
        self.switch_to(self.tabs.len() - 1, window, cx);
        // The strip's children are the tabs and then the + button: reveal
        // the + and with it the new tab just before it.
        self.tab_scroll.scroll_to_item(self.tabs.len());
        self.mark_dirty(cx);
    }

    /// Deletes circuit `index` after confirmation; the last circuit stays.
    /// The question is answered asynchronously (D25), so the circuit is
    /// found again by its tab when the answer arrives.
    pub(super) fn delete_circuit(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_asking() || !self.can_delete_circuit(index) {
            return;
        }
        let id = self.tabs[index].id;
        let name = self.doc.project.circuits[index].name.clone();
        self.ask(
            move |prompts, window, cx| prompts.confirm_delete_circuit(&name, window, cx),
            move |this, confirmed, window, cx| {
                // While the question was open the project may have been
                // replaced or its circuits changed.
                let index = this
                    .tab_index(id)
                    .filter(|&index| confirmed && this.can_delete_circuit(index));
                if let Some(index) = index {
                    this.remove_circuit(index, window, cx);
                }
            },
            window,
            cx,
        );
    }

    /// Whether circuit `index` exists and may be deleted: the project is
    /// editable and keeps at least one circuit.
    fn can_delete_circuit(&self, index: usize) -> bool {
        !self.doc.read_only && self.tabs.len() > 1 && index < self.tabs.len()
    }

    /// Removes circuit `index` with its results and problems, keeping the
    /// shown circuit shown when it is another one.
    pub(super) fn remove_circuit(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = self.tabs.remove(index);
        self.doc.project.circuits.remove(index);
        self.diagnostics.clear_circuit(tab.id);
        if self
            .renaming
            .as_ref()
            .is_some_and(|rename| rename.tab == tab.id)
        {
            self.renaming = None;
        }
        let active = self.doc.project.active_circuit;
        let active = if active > index || active >= self.tabs.len() {
            active.saturating_sub(1)
        } else {
            active
        };
        self.switch_to(active, window, cx);
        self.mark_dirty(cx);
    }

    /// Starts renaming circuit `index` in place (double-click on its tab).
    pub(super) fn start_rename(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.doc.read_only || index >= self.tabs.len() {
            return;
        }
        let name = self.doc.project.circuits[index].name.clone();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.commit_rename(true, window, cx),
                InputEvent::Blur => this.commit_rename(false, window, cx),
                InputEvent::Change | InputEvent::Focus => {}
            });
        input.focus_handle(cx).focus(window, cx);
        self.renaming = Some(Rename {
            tab: self.tabs[index].id,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Applies the typed name. An invalid name keeps the editor open after
    /// Enter and is dropped when focus leaves.
    pub(super) fn commit_rename(
        &mut self,
        explicit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(rename) = &self.renaming else {
            return;
        };
        let Some(index) = self.tab_index(rename.tab) else {
            self.renaming = None;
            return;
        };
        let typed = rename.input.read(cx).value().to_string();
        let others: Vec<&str> = self
            .doc
            .project
            .circuits
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(_, c)| c.name.as_str())
            .collect();
        match validate_circuit_name(&typed, &others) {
            Ok(name) => {
                self.renaming = None;
                if self.doc.project.circuits[index].name != name {
                    self.doc.project.circuits[index].name = name;
                    self.mark_dirty(cx);
                }
                self.focus_editor(window, cx);
            }
            Err(_) if explicit => {}
            Err(_) => self.renaming = None,
        }
        cx.notify();
    }

    /// Index of the circuit being renamed, if any.
    pub(super) fn renaming_index(&self) -> Option<usize> {
        self.renaming
            .as_ref()
            .and_then(|rename| self.tab_index(rename.tab))
    }

    /// Abandons an inline rename (Escape).
    pub(super) fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.renaming.take().is_some() {
            self.focus_editor(window, cx);
            cx.notify();
        }
    }
}

/// The trimmed name, or why it cannot be used.
pub(super) fn validate_circuit_name(typed: &str, others: &[&str]) -> Result<String, String> {
    let name = typed.trim();
    if name.is_empty() {
        Err("A circuit needs a name".into())
    } else if others.contains(&name) {
        Err(format!("There is already a circuit named \"{name}\""))
    } else {
        Ok(name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::validate_circuit_name;

    #[test]
    fn names_are_trimmed_non_empty_and_unique() {
        assert_eq!(
            validate_circuit_name("  Filter ", &["Circuit 1"]),
            Ok("Filter".into())
        );
        assert!(validate_circuit_name("   ", &[]).is_err());
        assert!(validate_circuit_name("Circuit 1", &["Circuit 1"]).is_err());
    }
}
