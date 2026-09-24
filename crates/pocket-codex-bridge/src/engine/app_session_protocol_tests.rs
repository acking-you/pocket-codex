use super::*;

#[test]
fn resume_reads_collaboration_mode_and_tolerates_legacy_responses() {
    for (mode, expected) in [
        (json!({"mode": "plan", "settings": {"model": "current-model"}}), Some("plan")),
        (json!({"mode": "default", "settings": {}}), Some("default")),
        (json!("plan"), Some("plan")),
        (Value::Null, None),
    ] {
        let parsed = runtime_config_from_response(&json!({"collaborationMode": mode}));
        assert_eq!(parsed.collaboration_mode.as_deref(), expected);
    }
    assert_eq!(runtime_config_from_response(&json!({})).collaboration_mode, None);
}

#[test]
fn image_references_preserve_inline_local_and_opaque_attachments() {
    let item = json!({"id": "user-1", "type": "userMessage", "content": [
        {"type": "text", "text": "Compare these images"},
        {"type": "image", "url": "data:image/png;base64,AA=="},
        {"type": "image", "fileId": "file-123", "detail": "original"},
        {"type": "localImage", "path": "/host/image.png"}
    ]});
    let parsed = parse_item(&item).expect("user message");
    assert_eq!(parsed.text, "Compare these images");
    assert_eq!(parsed.images, [
        "data:image/png;base64,AA==",
        "codex-file:file-123",
        "/host/image.png"
    ]);
}

#[test]
fn account_read_distinguishes_signed_out_and_supported_auth_methods() {
    for (response, expected) in [
        (json!({"account": null, "requiresOpenaiAuth": true}), (false, None)),
        (
            json!({"account": {"type": "chatgpt", "email": null, "planType": "pro"}}),
            (true, Some("chatgpt".into())),
        ),
        (json!({"account": {"type": "apiKey"}}), (true, Some("apikey".into()))),
        (json!({"account": {"type": "amazonBedrock"}}), (true, Some("amazonBedrock".into()))),
    ] {
        assert_eq!(auth_status_from_response(&response), expected);
    }
}

#[test]
fn thread_metadata_refreshes_model_and_clears_unset_effort() {
    let cached = ThreadRuntimeConfig {
        model: Some("old-model".into()),
        reasoning_effort: Some("high".into()),
        approval_policy: Some("on-request".into()),
        ..Default::default()
    };
    let current = runtime_config_with_thread(
        cached.clone(),
        Some(&json!({
            "model": "current-model", "reasoningEffort": null, "modelProvider": "openai"
        })),
    );
    assert_eq!(current.model.as_deref(), Some("current-model"));
    assert_eq!(current.reasoning_effort, None);
    assert_eq!(current.approval_policy, cached.approval_policy);
    let legacy = runtime_config_with_thread(cached, Some(&json!({"id": "thread-1"})));
    assert_eq!(legacy.model.as_deref(), Some("old-model"));
    assert_eq!(legacy.reasoning_effort.as_deref(), Some("high"));
}

#[test]
fn async_questions_survive_live_buffering_but_history_does_not_reopen_them() {
    let questions = json!([{"title": "Which platform?", "options": ["macOS", "Windows"]}]);
    let item = json!({
        "id": "question-1", "type": "agentMessage", "text": "", "delivery": "async",
        "questions": questions,
    });
    let history = flatten_turns(&[json!({"id": "turn-1", "items": [item.clone()]})]);
    let transcript = Mutex::new(HashMap::new());
    buffer_item(&transcript, &Inbound {
        method: "item/completed".into(),
        params: Some(json!({"threadId": "thread-1", "turnId": "turn-1", "item": item})),
        request_id: None,
    });
    let buffered = transcript.lock().expect("transcript lock");
    assert_eq!(history[0].questions_json, None);
    assert_eq!(buffered["thread-1"][0].questions_json, Some(questions.to_string()));
}
