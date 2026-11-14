use std::{
    io::{Cursor, Read},
    time::Duration,
};

use crate::{
    MatterportError,
    convert::scene_from_response,
    graphql::{
        GraphQlRequest, GraphQlResponse, MATTERPAK_QUERY, MESH_ASSETS_QUERY, MODEL_QUERY,
        MatterPakResponse, MeshAssetsResponse, Variables,
    },
};
use tflop_world_core::Scene;

const MODEL_API_ENDPOINT: &str = "https://api.matterport.com/api/models/graph";
/// The MatterPak is large enough that the default request timeout is too short.
const MATTERPAK_TIMEOUT: Duration = Duration::from_secs(600);

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

    /// Downloads the model's defurnished mesh: an untextured, Z-up OBJ of the
    /// space with furniture removed. Supports both legacy direct assets and
    /// the newer defurnished-view model. Returns `None` if neither exists.
    pub async fn fetch_defurnished_mesh(
        &self,
        model_id: &str,
    ) -> Result<Option<Vec<u8>>, MatterportError> {
        let request = GraphQlRequest {
            query: MESH_ASSETS_QUERY,
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
            .json::<MeshAssetsResponse>()
            .await?;

        if !response.errors.is_empty() {
            let messages = response
                .errors
                .into_iter()
                .map(|error| error.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(MatterportError::GraphQl(messages));
        }

        let model = response
            .data
            .and_then(|data| data.model)
            .ok_or_else(|| MatterportError::MissingModel(model_id.to_owned()))?;
        let direct_url = model
            .assets
            .into_iter()
            .flat_map(|assets| assets.meshes)
            .find(|mesh| {
                mesh.filename
                    .as_deref()
                    .is_some_and(|name| name.starts_with("defurnished") && name.ends_with(".obj"))
            })
            .and_then(|mesh| mesh.url);
        let view_url = model
            .views
            .into_iter()
            .filter_map(|view| view.model)
            .filter_map(|model| model.assets)
            .flat_map(|assets| assets.meshes)
            .find(|mesh| {
                mesh.filename
                    .as_deref()
                    .is_some_and(|name| name.to_ascii_lowercase().ends_with(".obj"))
            })
            .and_then(|mesh| mesh.url);
        let url = direct_url.or(view_url);
        let Some(url) = url else {
            return Ok(None);
        };

        // The signed URL is a bearer credential; keep it out of errors.
        let bytes = self
            .http
            .get(&url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(reqwest::Error::without_url)?
            .bytes()
            .await
            .map_err(reqwest::Error::without_url)?;

        Ok(Some(bytes.to_vec()))
    }

    /// Downloads the model's MatterPak and returns the furnished mesh inside
    /// it: an OBJ of the full scan, Z-up, in the same frame as the
    /// defurnished mesh and room boundaries. The zip is about 140 MB, most of
    /// it a point cloud that is skipped.
    pub async fn fetch_furnished_mesh(&self, model_id: &str) -> Result<Vec<u8>, MatterportError> {
        let request = GraphQlRequest {
            query: MATTERPAK_QUERY,
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
            .json::<MatterPakResponse>()
            .await?;

        if !response.errors.is_empty() {
            let messages = response
                .errors
                .into_iter()
                .map(|error| error.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(MatterportError::GraphQl(messages));
        }

        let bundle = response
            .data
            .and_then(|data| data.model)
            .ok_or_else(|| MatterportError::MissingModel(model_id.to_owned()))?
            .bundle;
        let availability = bundle
            .as_ref()
            .and_then(|bundle| bundle.availability.clone())
            .unwrap_or_else(|| "missing".to_owned());
        let url = bundle
            .filter(|_| availability == "unlocked")
            .and_then(|bundle| bundle.assets.into_iter().find_map(|asset| asset.url))
            .ok_or_else(|| MatterportError::MatterPakUnavailable {
                model_id: model_id.to_owned(),
                availability,
            })?;

        // The signed URL is a bearer credential; keep it out of errors.
        let archive = self
            .http
            .get(&url)
            .timeout(MATTERPAK_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(reqwest::Error::without_url)?
            .bytes()
            .await
            .map_err(reqwest::Error::without_url)?;

        let mut zip = zip::ZipArchive::new(Cursor::new(archive))?;
        let mut mesh_index = None;
        for index in 0..zip.len() {
            if let Some(Ok(name)) = zip.name_for_index(index)
                && name.to_ascii_lowercase().ends_with(".obj")
            {
                mesh_index = Some(index);
                break;
            }
        }
        let mut entry = zip.by_index(mesh_index.ok_or(MatterportError::MatterPakMissingMesh)?)?;
        let mut obj = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or_default());
        entry
            .read_to_end(&mut obj)
            .map_err(|error| MatterportError::MatterPakArchive(error.into()))?;
        Ok(obj)
    }
}
