mod compatible;
mod gemini;
mod http;
mod openai;
mod xai;

pub mod capabilities;
pub mod error;
pub mod types;

use std::future::Future;
use std::pin::Pin;

pub use compatible::OpenAiCompatibleAdapter;
pub use gemini::GeminiAdapter;
pub use openai::OpenAiAdapter;
pub use xai::XaiAdapter;

use error::ProviderError;
use types::{
    AssetSource, BatchSubmission, ConnectionTest, DiscoveredModel, DownloadedAsset, EventHandler,
    GenerationRequest, GenerationResponse, JobPollResult, ProviderConfig, ProviderCredentials,
    ProviderKind, RemoteJob,
};

pub type ProviderFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, ProviderError>> + Send + 'a>>;

pub trait ProviderAdapter: Send + Sync {
    fn kind(&self) -> ProviderKind;

    fn config(&self) -> &ProviderConfig;

    fn test_connection(&self) -> ProviderFuture<'_, ConnectionTest>;

    fn list_models(&self) -> ProviderFuture<'_, Vec<DiscoveredModel>>;

    fn validate(&self, request: &GenerationRequest) -> Result<(), ProviderError>;

    fn execute<'a>(
        &'a self,
        request: &'a GenerationRequest,
    ) -> ProviderFuture<'a, GenerationResponse>;

    fn execute_stream<'a>(
        &'a self,
        _request: &'a GenerationRequest,
        _events: EventHandler,
    ) -> ProviderFuture<'a, GenerationResponse> {
        Box::pin(async move {
            Err(ProviderError::unsupported(format!(
                "streaming is not implemented for {:?}",
                self.kind()
            )))
        })
    }

    fn submit_batch<'a>(
        &'a self,
        _submission: &'a BatchSubmission,
    ) -> ProviderFuture<'a, RemoteJob> {
        Box::pin(async move {
            Err(ProviderError::unsupported(format!(
                "provider batch is not implemented for {:?}",
                self.kind()
            )))
        })
    }

    fn poll_job<'a>(&'a self, _job: &'a RemoteJob) -> ProviderFuture<'a, JobPollResult> {
        Box::pin(async move {
            Err(ProviderError::unsupported(format!(
                "remote job polling is not implemented for {:?}",
                self.kind()
            )))
        })
    }

    fn cancel_job<'a>(&'a self, _job: &'a RemoteJob) -> ProviderFuture<'a, RemoteJob> {
        Box::pin(async move {
            Err(ProviderError::unsupported(format!(
                "remote job cancellation is not implemented for {:?}",
                self.kind()
            )))
        })
    }

    fn delete_file<'a>(&'a self, _file_id: &'a str) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            Err(ProviderError::unsupported(format!(
                "remote file deletion is not implemented for {:?}",
                self.kind()
            )))
        })
    }

    fn download_asset<'a>(&'a self, source: &'a AssetSource)
    -> ProviderFuture<'a, DownloadedAsset>;
}

pub fn create_adapter(
    config: ProviderConfig,
    credentials: ProviderCredentials,
) -> Result<Box<dyn ProviderAdapter>, ProviderError> {
    match config.kind {
        ProviderKind::OpenAi => Ok(Box::new(OpenAiAdapter::new(config, credentials)?)),
        ProviderKind::Xai => Ok(Box::new(XaiAdapter::new(config, credentials)?)),
        ProviderKind::Gemini => Ok(Box::new(GeminiAdapter::new(config, credentials)?)),
        ProviderKind::OpenAiCompatible => {
            Ok(Box::new(OpenAiCompatibleAdapter::new(config, credentials)?))
        }
    }
}
