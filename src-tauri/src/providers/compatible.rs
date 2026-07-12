use super::error::ProviderError;
use super::openai::OpenAiAdapter;
use super::types::{
    AssetSource, BatchSubmission, ConnectionTest, DiscoveredModel, DownloadedAsset, EventHandler,
    GenerationRequest, GenerationResponse, JobPollResult, ProviderConfig, ProviderCredentials,
    ProviderKind, RemoteJob,
};
use super::{ProviderAdapter, ProviderFuture};

pub struct OpenAiCompatibleAdapter {
    inner: OpenAiAdapter,
}

impl OpenAiCompatibleAdapter {
    pub fn new(
        config: ProviderConfig,
        credentials: ProviderCredentials,
    ) -> Result<Self, ProviderError> {
        Ok(Self {
            inner: OpenAiAdapter::new_with_kind(
                config,
                credentials,
                ProviderKind::OpenAiCompatible,
            )?,
        })
    }
}

impl ProviderAdapter for OpenAiCompatibleAdapter {
    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenAiCompatible
    }

    fn config(&self) -> &ProviderConfig {
        self.inner.config()
    }

    fn test_connection(&self) -> ProviderFuture<'_, ConnectionTest> {
        self.inner.test_connection()
    }

    fn list_models(&self) -> ProviderFuture<'_, Vec<DiscoveredModel>> {
        self.inner.list_models()
    }

    fn validate(&self, request: &GenerationRequest) -> Result<(), ProviderError> {
        self.inner.validate(request)
    }

    fn execute<'a>(
        &'a self,
        request: &'a GenerationRequest,
    ) -> ProviderFuture<'a, GenerationResponse> {
        self.inner.execute(request)
    }

    fn execute_stream<'a>(
        &'a self,
        request: &'a GenerationRequest,
        events: EventHandler,
    ) -> ProviderFuture<'a, GenerationResponse> {
        self.inner.execute_stream(request, events)
    }

    fn submit_batch<'a>(
        &'a self,
        submission: &'a BatchSubmission,
    ) -> ProviderFuture<'a, RemoteJob> {
        self.inner.submit_batch(submission)
    }

    fn poll_job<'a>(&'a self, job: &'a RemoteJob) -> ProviderFuture<'a, JobPollResult> {
        self.inner.poll_job(job)
    }

    fn cancel_job<'a>(&'a self, job: &'a RemoteJob) -> ProviderFuture<'a, RemoteJob> {
        self.inner.cancel_job(job)
    }

    fn delete_file<'a>(&'a self, file_id: &'a str) -> ProviderFuture<'a, ()> {
        self.inner.delete_file(file_id)
    }

    fn download_asset<'a>(
        &'a self,
        source: &'a AssetSource,
    ) -> ProviderFuture<'a, DownloadedAsset> {
        self.inner.download_asset(source)
    }
}
