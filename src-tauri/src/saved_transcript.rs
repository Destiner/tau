pub use tau_core::saved_transcript::SavedTranscript;

#[tauri::command]
pub async fn read_saved_transcript(
    project_path: String,
    session_id: String,
    session_path: String,
) -> SavedTranscript {
    let generation = tau_core::saved_transcript::begin_saved_transcript_read();
    tauri::async_runtime::spawn_blocking(move || {
        tau_core::saved_transcript::read_saved_transcript_at_generation(
            project_path,
            session_id,
            session_path,
            generation,
        )
    })
    .await
    .unwrap_or(SavedTranscript { messages: None })
}

#[tauri::command]
pub fn cancel_saved_transcript() {
    tau_core::saved_transcript::cancel_saved_transcript();
}
