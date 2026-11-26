use super::*;

#[test]
fn every_word_of_the_query_must_appear() {
    assert!(matches("Ripple delete gap (Ctrl+Delete)", "delete rip"));
    assert!(matches("Riproduzione in loop", "LOOP"));
    assert!(!matches("Insert edit", "overwrite"));
    assert!(matches("anything", ""));
}

#[test]
fn the_palette_lists_actions_and_commands_but_not_itself() {
    let app = VenturiApp::default();
    let entries = app.palette_entries();
    assert!(
        entries
            .iter()
            .any(|(e, _)| *e == Entry::Action(Action::InsertEdit))
    );
    assert!(
        entries
            .iter()
            .any(|(e, _)| *e == Entry::Command(TimelineCommand::MatchColor))
    );
    assert!(
        !entries
            .iter()
            .any(|(e, _)| *e == Entry::Action(Action::CommandPalette))
    );
    assert!(entries.iter().all(|(_, label)| !label.is_empty()));
}
