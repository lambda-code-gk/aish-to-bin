//! 責務: backend worker entry を daemon 側境界として提供する。

use std::io::{self, BufReader};
use std::sync::{Arc, Mutex};

use ai::backend_api::{
    emit_lifecycle, emit_lifecycle_text, run_request, BackendApprovalHandler, BackendFrameEmitter,
};
use common::error::Error;
use daemon_api::{
    read_frame_sync, write_frame_sync, AiClientFrame, AiInteractionRequest, AiInteractionResponse,
    AiJobState, AiRunRequest, AiStreamFrame, SensitiveChoiceValue,
};

pub fn maybe_run_worker_from_env() -> Option<Result<i32, Error>> {
    if std::env::var_os("AISH_BACKEND_WORKER").is_some() {
        std::env::remove_var("AISH_BACKEND_WORKER");
        Some(run_worker_stdio())
    } else {
        None
    }
}

pub fn run_worker_stdio() -> Result<i32, Error> {
    let request: AiRunRequest = {
        let mut reader = BufReader::new(io::stdin());
        read_frame_sync(&mut reader).map_err(|e| Error::io_msg(e.to_string()))?
    };
    let lifecycle_request = request.clone();
    let job_id = request.job_id.clone();
    let parent_job_id = request.parent_job_id.clone();
    let emitter: Arc<dyn BackendFrameEmitter> = Arc::new(WorkerStdoutEmitter::new());
    let approval: Arc<dyn BackendApprovalHandler> = Arc::new(WorkerApprovalBridge::new(
        request.clone(),
        Arc::clone(&emitter),
    ));
    match run_request(request, Arc::clone(&emitter), approval) {
        Ok(exit_code) => {
            emitter.emit(AiStreamFrame::Lifecycle {
                job_id: job_id.clone(),
                parent_job_id: parent_job_id.clone(),
                state: AiJobState::Completed,
            })?;
            emit_lifecycle_text(&emitter, &lifecycle_request, AiJobState::Completed)?;
            emitter.emit(AiStreamFrame::Completed { job_id, exit_code })?;
            Ok(exit_code)
        }
        Err(err) => {
            emitter.emit(AiStreamFrame::Lifecycle {
                job_id: job_id.clone(),
                parent_job_id,
                state: AiJobState::Failed,
            })?;
            emit_lifecycle_text(&emitter, &lifecycle_request, AiJobState::Failed)?;
            emitter.emit(AiStreamFrame::Failed {
                job_id,
                message: err.to_string(),
            })?;
            Err(err)
        }
    }
}

struct WorkerStdoutEmitter {
    writer: Mutex<io::Stdout>,
}

impl WorkerStdoutEmitter {
    fn new() -> Self {
        Self {
            writer: Mutex::new(io::stdout()),
        }
    }
}

impl BackendFrameEmitter for WorkerStdoutEmitter {
    fn emit(&self, frame: AiStreamFrame) -> Result<(), Error> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| Error::system("worker stdout lock poisoned"))?;
        write_frame_sync(&mut *writer, &frame).map_err(|e| Error::io_msg(e.to_string()))
    }
}

struct WorkerApprovalBridge {
    request: AiRunRequest,
    emitter: Arc<dyn BackendFrameEmitter>,
    job_id: String,
    reader: Mutex<BufReader<io::Stdin>>,
    writer: Mutex<io::Stdout>,
}

impl WorkerApprovalBridge {
    fn new(request: AiRunRequest, emitter: Arc<dyn BackendFrameEmitter>) -> Self {
        let job_id = request.job_id.clone();
        Self {
            request,
            emitter,
            job_id,
            reader: Mutex::new(BufReader::new(io::stdin())),
            writer: Mutex::new(io::stdout()),
        }
    }

    fn round_trip(&self, request: AiInteractionRequest) -> Result<AiInteractionResponse, Error> {
        emit_lifecycle(&self.emitter, &self.request, AiJobState::WaitingInteraction)?;
        emit_lifecycle_text(&self.emitter, &self.request, AiJobState::WaitingInteraction)?;
        {
            let mut writer = self
                .writer
                .lock()
                .map_err(|_| Error::system("worker stdout lock poisoned"))?;
            write_frame_sync(
                &mut *writer,
                &AiStreamFrame::InteractionRequest {
                    job_id: self.job_id.clone(),
                    request,
                },
            )
            .map_err(|e| Error::io_msg(e.to_string()))?;
        }
        let mut reader = self
            .reader
            .lock()
            .map_err(|_| Error::system("worker stdin lock poisoned"))?;
        match read_frame_sync::<_, AiClientFrame>(&mut *reader)
            .map_err(|e| Error::io_msg(e.to_string()))?
        {
            AiClientFrame::InteractionResponse { job_id, response } => {
                if job_id != self.job_id {
                    return Err(Error::io_msg(format!(
                        "unexpected interaction response job_id: {}",
                        job_id
                    )));
                }
                Ok(response)
            }
        }
    }
}

impl BackendApprovalHandler for WorkerApprovalBridge {
    fn request_approval(&self, prompt: &str) -> Result<bool, Error> {
        match self.round_trip(AiInteractionRequest::Approval {
            prompt: prompt.to_string(),
        })? {
            AiInteractionResponse::Approval { approved } => Ok(approved),
            other => Err(Error::io_msg(format!(
                "unexpected worker approval response: {:?}",
                other
            ))),
        }
    }

    fn request_continue(&self, prompt: &str) -> Result<bool, Error> {
        match self.round_trip(AiInteractionRequest::Continue {
            prompt: prompt.to_string(),
        })? {
            AiInteractionResponse::Continue { continue_ } => Ok(continue_),
            other => Err(Error::io_msg(format!(
                "unexpected worker continue response: {:?}",
                other
            ))),
        }
    }

    fn request_sensitive_choice(
        &self,
        verbose_output: &str,
    ) -> Result<SensitiveChoiceValue, Error> {
        match self.round_trip(AiInteractionRequest::SensitivePrompt {
            verbose_output: verbose_output.to_string(),
        })? {
            AiInteractionResponse::SensitivePrompt { choice } => Ok(choice),
            other => Err(Error::io_msg(format!(
                "unexpected worker sensitive response: {:?}",
                other
            ))),
        }
    }
}
