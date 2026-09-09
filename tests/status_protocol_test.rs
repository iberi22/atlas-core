use atlas::protocol::SwalStatusCode;

#[test]
fn test_status_code_numeric_roundtrip() {
    let code = SwalStatusCode::TskRunning;
    assert_eq!(code.code(), 103);
    assert_eq!(code.slug(), "TSK_RUNNING");

    let restored = SwalStatusCode::from_code(103).expect("Should parse 103");
    assert_eq!(restored, SwalStatusCode::TskRunning);

    assert_eq!(SwalStatusCode::from_code(9999), None);
}

#[test]
fn test_status_code_transitions() {
    // Valid lifecycle flow
    assert!(SwalStatusCode::can_transition(
        SwalStatusCode::TskBacklog,
        SwalStatusCode::TskReady
    ));
    assert!(SwalStatusCode::can_transition(
        SwalStatusCode::TskReady,
        SwalStatusCode::TskDispatched
    ));
    assert!(SwalStatusCode::can_transition(
        SwalStatusCode::TskDispatched,
        SwalStatusCode::TskRunning
    ));
    assert!(SwalStatusCode::can_transition(
        SwalStatusCode::TskRunning,
        SwalStatusCode::TskInReview
    ));
    assert!(SwalStatusCode::can_transition(
        SwalStatusCode::TskInReview,
        SwalStatusCode::TskCompleted
    ));

    // Invalid skip transition
    assert!(!SwalStatusCode::can_transition(
        SwalStatusCode::TskBacklog,
        SwalStatusCode::TskCompleted
    ));
    assert!(!SwalStatusCode::can_transition(
        SwalStatusCode::TskDispatched,
        SwalStatusCode::TskCompleted
    ));
}

#[test]
fn test_incident_and_snippet_ranges() {
    let sev0 = SwalStatusCode::IncSev0Critical;
    assert_eq!(sev0.code(), 500);

    let snippet = SwalStatusCode::SnpFlutterBgService;
    assert_eq!(snippet.code(), 901);
    assert_eq!(snippet.slug(), "SNP_FLUTTER_BG_SERVICE");
}
