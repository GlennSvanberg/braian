//! Rule-based task boundaries. The browser uses the same rules. A classifier
//! may relabel a finished segment; it does not run on each event.

use std::collections::HashMap;

use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind {
    Human,
    Agent,
    Integration,
}

impl ActorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Agent => "agent",
            Self::Integration => "integration",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "human" => Some(Self::Human),
            "agent" => Some(Self::Agent),
            "integration" => Some(Self::Integration),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Explicit {
    Import,
    MassEdit,
    Command,
    Search,
}

#[derive(Debug, Clone)]
pub struct Observe {
    pub at_ms: i64,
    pub operation: String,
    pub actor: ActorKind,
    pub object_ids: Vec<String>,
    pub explicit: Option<Explicit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub task_id: String,
    pub task_type: String,
    pub objects: Vec<String>,
    pub last_at_ms: i64,
    pub saw_search: bool,
    pub sticky: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Step {
    started: bool,
    ambiguous: bool,
    task_type: String,
    objects: Vec<String>,
    last_at_ms: i64,
    saw_search: bool,
    sticky: bool,
}

pub const TASK_TYPES: &[&str] = &[
    "import",
    "mass_edit",
    "enrich",
    "search",
    "navigate",
    "review",
    "model",
    "agent",
    "other",
];

pub fn canonical_task_type(value: &str) -> &str {
    if TASK_TYPES.contains(&value) {
        value
    } else {
        "other"
    }
}

pub fn is_change(operation: &str) -> bool {
    matches!(
        operation,
        "create_object"
            | "patch_object"
            | "mass_edit"
            | "rollback"
            | "set_relation"
            | "accept_proposal"
            | "bulk"
            | "import"
    ) || operation.starts_with("model.")
}

/// Applies one operation to the open task.
///
/// `new_id` is used only when the rules start a task. The returned bool is
/// the ambiguous flag for this event.
pub fn apply(
    cursor: Option<&Cursor>,
    event: &Observe,
    gap_ms: i64,
    mut new_id: impl FnMut() -> String,
) -> (Cursor, bool) {
    let step = advance(cursor, event, gap_ms);
    let task_id = if step.started {
        new_id()
    } else if let Some(cursor) = cursor {
        cursor.task_id.clone()
    } else {
        new_id()
    };
    (
        Cursor {
            task_id,
            task_type: step.task_type,
            objects: step.objects,
            last_at_ms: step.last_at_ms,
            saw_search: step.saw_search,
            sticky: step.sticky,
        },
        step.ambiguous,
    )
}

fn advance(cursor: Option<&Cursor>, event: &Observe, gap_ms: i64) -> Step {
    let proposed = task_type_for(event);
    let stale = match cursor {
        None => true,
        Some(cursor) => event.at_ms.saturating_sub(cursor.last_at_ms) > gap_ms,
    };
    if event.actor == ActorKind::Agent {
        if let Some(cursor) = cursor.filter(|_| !stale) {
            let task_type = if is_change(&event.operation)
                && matches!(cursor.task_type.as_str(), "search" | "navigate" | "other")
            {
                "enrich".to_owned()
            } else {
                cursor.task_type.clone()
            };
            return cont(cursor, event, &task_type, false);
        }
        let task_type = if proposed == "other" {
            "agent".to_owned()
        } else {
            proposed
        };
        return start(event, &task_type, true);
    }
    if stale {
        return start(event, &proposed, false);
    }
    let Some(cursor) = cursor else {
        return start(event, &proposed, false);
    };
    let overlap = overlaps(&cursor.objects, &event.object_ids);
    let explicit_boundary = matches!(event.explicit, Some(Explicit::Import | Explicit::MassEdit))
        || matches!(event.operation.as_str(), "bulk" | "import" | "mass_edit");
    if explicit_boundary {
        let follow_up = cursor.sticky
            && overlap
            && !matches!(event.operation.as_str(), "bulk" | "import" | "mass_edit");
        if follow_up {
            return cont(cursor, event, &cursor.task_type, false);
        }
        return start(event, &proposed, false);
    }
    if matches!(event.explicit, Some(Explicit::Command)) {
        if overlap || (event.object_ids.is_empty() && cursor.sticky) {
            return cont(
                cursor,
                event,
                &merge_type(&cursor.task_type, &proposed),
                false,
            );
        }
        let task_type = if proposed == "navigate" {
            "agent"
        } else {
            proposed.as_str()
        };
        return start(event, task_type, false);
    }
    if cursor.saw_search
        && cursor.objects.is_empty()
        && (is_change(&event.operation)
            || event.operation == "get_object"
            || event.operation == "ui.select")
    {
        return cont(cursor, event, "enrich", false);
    }
    if cursor.sticky {
        if event.object_ids.is_empty() || overlap || cursor.objects.is_empty() {
            let ambiguous = event.operation == "search" && !event.object_ids.is_empty() && !overlap;
            return cont(cursor, event, &cursor.task_type, ambiguous);
        }
        return start(event, &proposed, true);
    }
    if overlap || event.object_ids.is_empty() {
        let task_type = if is_change(&event.operation)
            && matches!(cursor.task_type.as_str(), "search" | "navigate" | "other")
        {
            "enrich".to_owned()
        } else if proposed == "navigate" {
            cursor.task_type.clone()
        } else if cursor.task_type == "other" {
            proposed
        } else {
            cursor.task_type.clone()
        };
        return cont(cursor, event, &task_type, false);
    }
    let ambiguous = matches!(event.operation.as_str(), "get_object" | "ui.select");
    start(event, &proposed, ambiguous)
}

fn start(event: &Observe, task_type: &str, ambiguous: bool) -> Step {
    Step {
        started: true,
        ambiguous,
        task_type: task_type.to_owned(),
        objects: capped(&event.object_ids),
        last_at_ms: event.at_ms,
        saw_search: is_search(event),
        sticky: task_type == "import",
    }
}

fn cont(cursor: &Cursor, event: &Observe, task_type: &str, ambiguous: bool) -> Step {
    let mut objects = cursor.objects.clone();
    for id in &event.object_ids {
        if !objects.iter().any(|existing| existing == id) {
            objects.push(id.clone());
        }
    }
    if objects.len() > 64 {
        objects.truncate(64);
    }
    Step {
        started: false,
        ambiguous,
        task_type: task_type.to_owned(),
        objects,
        last_at_ms: event.at_ms,
        saw_search: cursor.saw_search || is_search(event),
        sticky: cursor.sticky || task_type == "import",
    }
}

fn task_type_for(event: &Observe) -> String {
    if matches!(event.explicit, Some(Explicit::Import))
        || matches!(event.operation.as_str(), "bulk" | "import")
    {
        return "import".into();
    }
    if matches!(event.explicit, Some(Explicit::MassEdit)) || event.operation == "mass_edit" {
        return "mass_edit".into();
    }
    if event.operation.starts_with("model.") {
        return "model".into();
    }
    if is_search(event) {
        return "search".into();
    }
    if matches!(
        event.operation.as_str(),
        "history" | "rollback" | "accept_proposal" | "discard_proposal" | "impact"
    ) {
        return "review".into();
    }
    if is_navigate(&event.operation) {
        return "navigate".into();
    }
    if event.operation.starts_with("agent.") {
        return "agent".into();
    }
    if matches!(
        event.operation.as_str(),
        "patch_object"
            | "create_object"
            | "set_relation"
            | "get_object"
            | "ui.select"
            | "ui.select_rows"
    ) {
        return "enrich".into();
    }
    if event.actor == ActorKind::Agent {
        return "agent".into();
    }
    "other".into()
}

fn is_search(event: &Observe) -> bool {
    matches!(event.explicit, Some(Explicit::Search))
        || matches!(
            event.operation.as_str(),
            "search" | "ui.filter" | "list_objects"
        )
}

fn is_navigate(operation: &str) -> bool {
    matches!(
        operation,
        "ui.open_view"
            | "ui.open_template"
            | "ui.open_workspace"
            | "ui.place"
            | "ui.link"
            | "ui.group"
            | "ui.sort"
            | "ui.show_column"
            | "ui.hide_column"
            | "ui.move_column"
            | "ui.set_theme"
            | "ui.export"
            | "open_view"
            | "close_view"
            | "move_view"
            | "move_view_left"
            | "move_view_right"
            | "move_view_up"
            | "move_view_down"
            | "split"
            | "resize"
            | "swap"
            | "focus"
            | "zoom"
            | "get_layout"
            | "reset_layout"
    )
}

fn overlaps(left: &[String], right: &[String]) -> bool {
    if left.is_empty() || right.is_empty() {
        return false;
    }
    right
        .iter()
        .any(|id| left.iter().any(|existing| existing == id))
}

fn capped(ids: &[String]) -> Vec<String> {
    ids.iter().take(64).cloned().collect()
}

fn merge_type(current: &str, proposed: &str) -> String {
    if current == "import" || current == "mass_edit" {
        return current.to_owned();
    }
    if proposed == "navigate" {
        return current.to_owned();
    }
    if matches!(current, "search" | "navigate" | "other")
        && matches!(proposed, "enrich" | "model" | "review" | "agent")
    {
        return proposed.to_owned();
    }
    current.to_owned()
}

/// In-memory cursors for callers that are not the browser (MCP, integrations).
/// The map key stays in the process. Events carry a session id, not a name.
#[derive(Debug)]
pub struct Assigner {
    gap_ms: i64,
    slots: HashMap<Uuid, Slot>,
}

#[derive(Debug)]
struct Slot {
    session_id: Uuid,
    cursor: Option<Cursor>,
}

#[derive(Debug)]
pub struct Assigned {
    pub task_id: Uuid,
    pub session_id: Uuid,
    pub task_type: String,
    pub ambiguous: bool,
}

impl Assigner {
    pub fn new(gap_ms: i64) -> Self {
        Self {
            gap_ms,
            slots: HashMap::new(),
        }
    }

    pub fn set_gap(&mut self, gap_ms: i64) {
        self.gap_ms = gap_ms;
    }

    pub fn observe(&mut self, principal: Uuid, event: &Observe) -> Assigned {
        let slot = self.slots.entry(principal).or_insert_with(|| Slot {
            session_id: Uuid::now_v7(),
            cursor: None,
        });
        let session_id = slot.session_id;
        let fresh = Uuid::now_v7();
        let (cursor, ambiguous) = apply(slot.cursor.as_ref(), event, self.gap_ms, || {
            fresh.to_string()
        });
        let task_id = Uuid::parse_str(&cursor.task_id).unwrap_or(fresh);
        slot.cursor = Some(cursor);
        Assigned {
            task_id,
            session_id,
            task_type: canonical_task_type(
                slot.cursor
                    .as_ref()
                    .map(|cursor| cursor.task_type.as_str())
                    .unwrap_or("other"),
            )
            .to_owned(),
            ambiguous,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        at: i64,
        operation: &str,
        actor: ActorKind,
        ids: &[&str],
        explicit: Option<Explicit>,
    ) -> Observe {
        Observe {
            at_ms: at,
            operation: operation.to_owned(),
            actor,
            object_ids: ids.iter().map(|id| (*id).to_owned()).collect(),
            explicit,
        }
    }

    fn play(events: &[Observe]) -> Vec<(String, String)> {
        let mut cursor = None;
        let mut seq = 0u32;
        let mut out = Vec::new();
        for event in events {
            let (next, _) = apply(cursor.as_ref(), event, 300_000, || {
                seq += 1;
                format!("t{seq}")
            });
            out.push((next.task_id.clone(), next.task_type.clone()));
            cursor = Some(next);
        }
        out
    }

    #[test]
    fn search_then_edit_is_one_enrich_task() {
        let steps = play(&[
            event(0, "search", ActorKind::Human, &[], Some(Explicit::Search)),
            event(2_000, "get_object", ActorKind::Human, &["a"], None),
            event(4_000, "patch_object", ActorKind::Human, &["a"], None),
        ]);
        assert_eq!(steps[0].1, "search");
        assert_eq!(steps[1].0, steps[0].0);
        assert_eq!(steps[1].1, "enrich");
        assert_eq!(steps[2].0, steps[0].0);
        assert_eq!(steps[2].1, "enrich");
    }

    #[test]
    fn import_then_edits_on_those_objects_stay() {
        let steps = play(&[
            event(
                0,
                "bulk",
                ActorKind::Integration,
                &["a", "b"],
                Some(Explicit::Import),
            ),
            event(5_000, "patch_object", ActorKind::Human, &["b"], None),
            event(6_000, "ui.open_view", ActorKind::Human, &[], None),
        ]);
        assert_eq!(steps[0].1, "import");
        assert_eq!(steps[1].0, steps[0].0);
        assert_eq!(steps[2].0, steps[0].0);
        assert_eq!(steps[2].1, "import");
    }

    #[test]
    fn mass_edit_starts_its_own_task() {
        let steps = play(&[
            event(0, "bulk", ActorKind::Human, &["a"], Some(Explicit::Import)),
            event(
                1_000,
                "mass_edit",
                ActorKind::Human,
                &["a"],
                Some(Explicit::MassEdit),
            ),
        ]);
        assert_ne!(steps[0].0, steps[1].0);
        assert_eq!(steps[1].1, "mass_edit");
    }

    #[test]
    fn unrelated_objects_end_the_task() {
        let steps = play(&[
            event(0, "get_object", ActorKind::Human, &["a"], None),
            event(1_000, "patch_object", ActorKind::Human, &["a"], None),
            event(2_000, "get_object", ActorKind::Human, &["z"], None),
        ]);
        assert_eq!(steps[0].0, steps[1].0);
        assert_ne!(steps[1].0, steps[2].0);
    }

    #[test]
    fn inactivity_gap_starts_a_new_task() {
        let steps = play(&[
            event(0, "patch_object", ActorKind::Human, &["a"], None),
            event(300_001, "patch_object", ActorKind::Human, &["a"], None),
        ]);
        assert_ne!(steps[0].0, steps[1].0);
    }

    #[test]
    fn agent_steps_join_the_human_task() {
        let steps = play(&[
            event(0, "search", ActorKind::Human, &[], Some(Explicit::Search)),
            event(500, "ui.filter", ActorKind::Agent, &[], None),
            event(800, "patch_object", ActorKind::Agent, &["a"], None),
        ]);
        assert_eq!(steps[0].0, steps[1].0);
        assert_eq!(steps[1].0, steps[2].0);
        assert_eq!(steps[2].1, "enrich");
    }

    #[test]
    fn a_command_on_other_objects_starts_a_task() {
        let steps = play(&[
            event(0, "get_object", ActorKind::Human, &["a"], None),
            event(
                1_000,
                "ui.open_view",
                ActorKind::Human,
                &[],
                Some(Explicit::Command),
            ),
        ]);
        assert_ne!(steps[0].0, steps[1].0);
    }
}
