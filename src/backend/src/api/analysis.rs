use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};

use crate::{
    AppState,
    error::ApiError,
    model::{
        AnalysisRecommendationsResponse, AnalysisSummaryResponse, ColumnsAnalysisResponse,
        PagesAnalysisQuery, PagesAnalysisResponse, QueryCostRequest, QueryCostResponse,
        RowGroupsAnalysisResponse,
    },
};

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/datasets/{dataset_id}/analysis/summary",
            get(analysis_summary),
        )
        .route(
            "/datasets/{dataset_id}/analysis/columns",
            get(analysis_columns),
        )
        .route(
            "/datasets/{dataset_id}/analysis/row-groups",
            get(analysis_row_groups),
        )
        .route("/datasets/{dataset_id}/analysis/pages", get(analysis_pages))
        .route(
            "/datasets/{dataset_id}/analysis/query-cost",
            post(analysis_query_cost),
        )
        .route(
            "/datasets/{dataset_id}/analysis/recommendations",
            get(analysis_recommendations),
        )
}

async fn analysis_summary(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
) -> Result<Json<AnalysisSummaryResponse>, ApiError> {
    state
        .engine
        .analysis_summary(&dataset_id)
        .await
        .map(Json)
        .map_err(ApiError::from_engine)
}

async fn analysis_columns(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
) -> Result<Json<ColumnsAnalysisResponse>, ApiError> {
    state
        .engine
        .analysis_columns(&dataset_id)
        .await
        .map(Json)
        .map_err(ApiError::from_engine)
}

async fn analysis_row_groups(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
) -> Result<Json<RowGroupsAnalysisResponse>, ApiError> {
    state
        .engine
        .analysis_row_groups(&dataset_id)
        .await
        .map(Json)
        .map_err(ApiError::from_engine)
}

async fn analysis_pages(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
    Query(query): Query<PagesAnalysisQuery>,
) -> Result<Json<PagesAnalysisResponse>, ApiError> {
    state
        .engine
        .analysis_pages(&dataset_id, query)
        .await
        .map(Json)
        .map_err(ApiError::from_engine)
}

async fn analysis_query_cost(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
    Json(payload): Json<QueryCostRequest>,
) -> Result<Json<QueryCostResponse>, ApiError> {
    if payload.limit == 0 {
        return Err(ApiError::bad_request("limit must be at least 1"));
    }
    state
        .engine
        .analysis_query_cost(&dataset_id, payload)
        .await
        .map(Json)
        .map_err(ApiError::from_engine)
}

async fn analysis_recommendations(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
) -> Result<Json<AnalysisRecommendationsResponse>, ApiError> {
    state
        .engine
        .analysis_recommendations(&dataset_id)
        .await
        .map(Json)
        .map_err(ApiError::from_engine)
}
