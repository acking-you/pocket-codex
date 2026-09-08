use super::*;

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
