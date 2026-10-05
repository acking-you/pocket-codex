use super::*;

#[test]
fn persisted_command_truncation_survives_live_and_restored_items() {
    let output = "first line\n... command output truncated for persistence ...\nlast line\n";
    let item = json!({
        "type": "commandExecution", "id": "command", "command": "cargo test",
        "status": "completed", "aggregatedOutput": output, "exitCode": 0
    });
    let restored = parse_item(&item).expect("command history");
    let live = map_event(Inbound {
        method: "item/completed".into(),
        params: Some(json!({"threadId": "thread", "turnId": "turn", "item": item})),
        request_id: None,
    });
    assert_eq!(restored.text, format!("{output}\n[exit 0]"));
    assert_eq!(live.text.as_deref(), Some(restored.text.as_str()));
    assert_eq!(live.title.as_deref(), Some("cargo test"));
}

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
    let transcript = Mutex::new(LiveTranscript::default());
    buffer_item(&transcript, &Inbound {
        method: "item/completed".into(),
        params: Some(json!({"threadId": "thread-1", "turnId": "turn-1", "item": item})),
        request_id: None,
    });
    let buffered = transcript.lock().expect("transcript lock");
    assert_eq!(history[0].questions_json, None);
    assert_eq!(buffered.tail("thread-1", 100)[0].questions_json, Some(questions.to_string()));
}

#[test]
fn model_speed_capabilities_match_current_and_legacy_catalogs() {
    assert_eq!(
        parse_service_tiers(&json!({"serviceTiers": [{"id": "priority", "name": "Fast"}]})),
        ["priority"]
    );
    assert_eq!(parse_service_tiers(&json!({"additionalSpeedTiers": ["fast"]})), ["priority"]);
    assert!(parse_service_tiers(&json!({"id": "model-without-speed-metadata"})).is_empty());
    assert_eq!(parse_service_tiers(&json!({"serviceTiers": [{"id": "flex"}]})), ["flex"]);
}

#[test]
fn runtime_restores_reviewer_and_service_tier_without_conflating_approval_policy() {
    let resumed = runtime_config_from_response(&json!({
        "approvalPolicy": "on-request", "approvalsReviewer": "auto_review", "serviceTier": "priority"
    }));
    assert_eq!(resumed.approval_policy.as_deref(), Some("on-request"));
    assert_eq!(resumed.approvals_reviewer.as_deref(), Some("auto_review"));
    assert_eq!(resumed.service_tier.as_deref(), Some("priority"));
    let updated = runtime_config_from_settings(&json!({
        "approvalPolicy": "on-request", "approvalsReviewer": "user", "serviceTier": null
    }));
    assert_eq!(updated.approvals_reviewer.as_deref(), Some("user"));
    assert_eq!(updated.service_tier, None);
    assert_eq!(
        runtime_config_from_response(&json!({"approvalsReviewer": "guardian_subagent"}))
            .approvals_reviewer
            .as_deref(),
        Some("auto_review")
    );
}

#[test]
fn generated_images_survive_live_events_and_history_without_inlining_saved_files() {
    for (fields, expected) in [
        (json!({"status": "in_progress", "result": ""}), Vec::<String>::new()),
        (
            json!({"status": "completed", "savedPath": "/tmp/generated.png", "result": "LARGE"}),
            vec!["/tmp/generated.png".into()],
        ),
        (json!({"status": "completed", "result": "aW1hZ2U="}), vec!["data:image/png;base64,\
                                                                     aW1hZ2U="
            .into()]),
        (
            json!({"status": "failed", "result": "", "failure": {"type": "usageLimitExceeded", "limitId": "image"}}),
            vec![],
        ),
    ] {
        let mut item = fields;
        item["type"] = json!("imageGeneration");
        item["id"] = json!("generated-1");
        let history = parse_item(&item).expect("generated image");
        let event = map_event(Inbound {
            method: "item/completed".into(),
            params: Some(json!({"threadId": "thread", "turnId": "turn", "item": item})),
            request_id: None,
        });
        assert_eq!(history.images, expected);
        assert_eq!(event.images, expected);
        assert!(!history.text.contains("LARGE"));
        assert_eq!(event.item_type.as_deref(), Some("imageGeneration"));
    }
}

#[test]
#[ignore = "requires PCX_IMAGE_CAPTURE pointing to isolated native app-server events.jsonl"]
fn native_image_capture_maps_live_and_restored_artifacts() {
    let path = std::env::var("PCX_IMAGE_CAPTURE").expect("native capture");
    let capture = std::fs::read_to_string(path).expect("capture contents");
    let mut started = false;
    let mut completed = false;
    for line in capture.lines() {
        let value: Value = serde_json::from_str(line).expect("native event JSON");
        let item = &value["params"]["item"];
        if item["type"] != "imageGeneration" {
            continue;
        }
        let method = value["method"].as_str().expect("method");
        let event = map_event(Inbound {
            method: method.into(),
            params: Some(value["params"].clone()),
            request_id: None,
        });
        started |= method == "item/started";
        if method == "item/completed" {
            completed = true;
            let history = parse_item(item).expect("history item");
            assert_eq!(event.images, history.images);
            assert_eq!(event.images.len(), 1);
            assert!(std::path::Path::new(&event.images[0]).is_file());
            assert!(!history
                .text
                .contains(item["result"].as_str().expect("native result")));
        }
    }
    assert!(started && completed, "both native lifecycle edges must be captured");
}

#[test]
fn voice_classification_and_signaling_survive_the_bridge() {
    let meta = parse_thread_meta(&json!({"id": "voice", "threadSource": "pocket-codex-voice"}))
        .expect("voice metadata");
    assert_eq!(meta.thread_source.as_deref(), Some("pocket-codex-voice"));
    let cached = serde_json::to_value(&meta).expect("cache metadata");
    let restored: ThreadMeta = serde_json::from_value(cached).expect("restore metadata");
    assert_eq!(restored.thread_source, meta.thread_source);
    let event = map_event(Inbound {
        method: "thread/realtime/sdp".into(),
        params: Some(json!({"threadId": "voice", "sdp": "answer"})),
        request_id: None,
    });
    assert_eq!(event.thread_id.as_deref(), Some("voice"));
    assert_eq!(serde_json::from_str::<Value>(&event.raw).expect("event JSON")["sdp"], "answer");
    assert!(
        validate_realtime_request("thread/realtime/start", &json!({"threadId":"voice"})).is_ok()
    );
    assert!(validate_realtime_request("thread/realtime/start", &json!({})).is_err());
    assert!(validate_realtime_request(
        "thread/start",
        &json!({"threadSource":"pocket-codex-voice"})
    )
    .is_ok());
    assert!(validate_realtime_request("thread/start", &json!({})).is_err());
    assert!(validate_realtime_request("command/exec", &json!({})).is_err());
}

#[test]
fn guardian_lifecycle_retains_one_display_only_item_and_expands_history() {
    let transcript = Mutex::new(LiveTranscript::default());
    for status in ["inProgress", "approved", "denied", "timedOut", "aborted"] {
        let params = json!({"threadId":"parent", "turnId":"turn", "reviewId":"review",
            "targetItemId":"command", "review":{"status":status, "riskLevel":"high",
            "userAuthorization":"high", "rationale":"Explicitly authorized"},
            "action":{"type":"command", "command":"cargo test", "cwd":"/project"}});
        let inbound = Inbound {
            method: if status == "inProgress" {
                "item/autoApprovalReview/started".into()
            } else {
                "item/autoApprovalReview/completed".into()
            },
            params: Some(params.clone()),
            request_id: None,
        };
        buffer_item(&transcript, &inbound);
        let mapped = map_event(inbound);
        assert_eq!(mapped.item_id.as_deref(), Some("auto-review:review"));
        assert_eq!(mapped.item_type.as_deref(), Some("autoApprovalReview"));
        assert_eq!(mapped.thread_id.as_deref(), Some("parent"));
        assert!(mapped.request_id.is_none());
        let buffered = transcript.lock().expect("lock").tail("parent", 10);
        assert_eq!(buffered.len(), 1);
        assert_eq!(buffered[0].title, status);
        let page = json!({"data":[{"turnId":"turn", "item":{"type":"commandExecution",
            "id":"command", "command":"cargo test", "autoApprovalReviews":[params]}}]});
        let recovered = parse_prefetched_items(&page);
        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[0].id, "auto-review:review");
        assert_eq!(recovered[0].turn_id, "turn");
        assert_eq!(recovered[1].id, "command");
    }
    let metadata = parse_thread_meta(&json!({"id":"child", "parentThreadId":"parent",
        "threadSource":null, "source":{"subAgent":{"other":"guardian"}}}))
    .expect("metadata");
    assert_eq!(metadata.parent_thread_id.as_deref(), Some("parent"));
    assert_eq!(metadata.thread_source.as_deref(), Some("guardian_review"));
}
