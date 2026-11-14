use std::time::Duration;

use crate::{
    MatterportError,
    convert::scene_from_response,
    graphql::{GraphQlRequest, GraphQlResponse, MODEL_QUERY, Variables},
};
use tflop_world_core::Scene;

const MODEL_API_ENDPOINT: &str = "https://api.matterport.com/api/models/graph";

pub struct MatterportClient {
    http: reqwest::Client,
    token_id: String,
    token_secret: String,
}

impl MatterportClient {
    #[must_use]
    pub fn new(token_id: impl Into<String>, token_secret: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client configuration must be valid"),
            token_id: token_id.into(),
            token_secret: token_secret.into(),
        }
    }

    /// Fetches, converts, and validates one Matterport model.
    pub async fn fetch_scene(&self, model_id: &str) -> Result<Scene, MatterportError> {
        let request = GraphQlRequest {
            query: MODEL_QUERY,
            variables: Variables { id: model_id },
        };

        let response = self
            .http
            .post(MODEL_API_ENDPOINT)
            .basic_auth(&self.token_id, Some(&self.token_secret))
            .json(&request)
            .send()
            .await?
            .error_for_status()?
            .json::<GraphQlResponse>()
            .await?;

        scene_from_response(response, model_id)
    }
}
