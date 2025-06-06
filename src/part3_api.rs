// Part 3: Rate-Limited API Client Implementation (Advanced Difficulty)
// This component is our customer-facing API that must handle extreme traffic while maintaining reliability

use crate::circuit_breaker::CircuitBreaker;
use async_trait::async_trait;
use mock_server::MockServer;
// use parking_lot::RwLock;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicI16, AtomicU32, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use thiserror::Error;
use tokio::{sync::RwLock, time::Instant};

use tokio::{
    self,
    sync::{
        mpsc,
        oneshot::{self, Sender},
    },
};

// Enhanced error types for API client
#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Rate limit exceeded: {0}")]
    RateLimitExceeded(String, usize, usize),

    #[error("Request timeout after {0}ms")]
    Timeout(u64),

    #[error("Circuit breaker open for {service_name}")]
    CircuitBreakerOpen {
        service_name: String,
        retry_after_ms: Option<u64>,
    },

    #[error("API error: {status_code} - {message}")]
    ApiResponseError {
        status_code: u16,
        message: String,
        is_retryable: bool,
    },

    #[error("Preempted by higher priority request")]
    RequestPreempted,

    #[error("Client error: {0}")]
    ClientError(String),

    #[error("Request queue full")]
    QueueFull,

    #[error("Other error: {0}")]
    Other(String),
}

#[derive(Error, Debug)]
pub enum ClientError {
    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Initialization error: {0}")]
    InitError(String),
}

// Enhanced client configuration
#[derive(Debug, Clone, Default)]
pub struct ClientConfig {
    pub base_url: String,
    pub api_key: String,
    pub max_requests_per_second: u32,
    pub max_burst_size: u32,
    pub max_concurrent_requests: u32,
    pub timeout_ms: u64,
    pub retry_config: RetryConfig,
    pub circuit_breaker_config: CircuitBreakerConfig,
    pub queue_size_per_priority: usize,
    pub health_check_interval_ms: u64,
}

// Enhanced retry configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub backoff_multiplier: f64,
    pub jitter_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff_ms: 100,
            max_backoff_ms: 10000,
            backoff_multiplier: 2.0,
            jitter_factor: 0.1,
        }
    }
}

// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub success_threshold: u32,
    pub reset_timeout_ms: u64,
    pub half_open_max_requests: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            reset_timeout_ms: 4000,
            half_open_max_requests: 1,
        }
    }
}

// Request priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequestPriority {
    Low = 3,
    Medium = 2,
    High = 1,
    Critical = 0,
}

impl Default for RequestPriority {
    fn default() -> Self {
        RequestPriority::Medium
    }
}

// Enhanced client statistics
#[derive(Debug, Default)]
pub struct ClientStats {
    pub requests_sent: AtomicUsize,
    pub requests_succeeded: AtomicUsize,
    pub requests_failed: AtomicUsize,
    pub requests_throttled: AtomicUsize,
    pub requests_retried: AtomicUsize,
    pub requests_preempted: AtomicUsize,
    pub requests_timeout: usize,
    pub requests_circuit_broken: usize,
    pub average_response_time_ms: f64,
    pub p95_response_time_ms: f64,
    pub p99_response_time_ms: f64,
    pub max_response_time_ms: f64,
    pub active_requests: usize,
    pub queue_depth: usize,
    pub circuit_breaker_open: AtomicBool,
    pub current_rate_limit: AtomicU32,
    pub adaptive_rate_limit_multiplier: f64,
}

// Request and response types (enhanced for the assessment)
#[derive(Debug, Clone, Default)]
pub struct SearchRequest {
    pub hotel_ids: Vec<String>,
    pub check_in: String,
    pub check_out: String,
    pub guests: u32,
    pub priority: RequestPriority,
    pub idempotency_key: Option<String>,
    pub context: RequestContext,
    pub delay: Option<Instant>,
}

#[derive(Debug, Clone, Default)]
pub struct RequestContext {
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub correlation_id: String,
    pub client_info: Option<ClientInfo>,
    pub request_deadline: Option<std::time::SystemTime>,
}

#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub ip: String,
    pub user_agent: String,
    pub country: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SearchResponse {
    pub search_id: String,
    pub results: Vec<SearchResult>,
    pub rate_limit_remaining: Option<u32>,
    pub processing_time_ms: u64,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub hotel_id: String,
    pub available: bool,
    pub price: Option<f64>,
    pub currency: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BookingRequest {
    pub search_id: String,
    pub hotel_id: String,
    pub guest_name: String,
    pub payment_info: PaymentInfo,
    pub priority: RequestPriority,
    pub idempotency_key: String,
    pub context: RequestContext,
}

#[derive(Debug, Clone)]
pub struct PaymentInfo {
    pub card_type: String,
    pub last_four: String,
    pub expiry: String,
    pub token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BookingResponse {
    pub booking_id: String,
    pub status: String,
    pub confirmation_code: Option<String>,
    pub rate_limit_remaining: Option<u32>,
    pub processing_time_ms: u64,
}

// Health status for adaptively adjusting rate limits
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemHealth {
    Healthy,
    Degraded,
    Unhealthy,
}

// API client trait with enhanced requirements
#[async_trait]
pub trait ApiClient: Send + Sync + 'static {
    // Basic search operation
    async fn search(&self, request: SearchRequest) -> Result<SearchResponse, ApiError>;

    // Basic booking operation
    async fn book(&self, request: BookingRequest) -> Result<BookingResponse, ApiError>;

    // Get client statistics
    fn stats(&self) -> ClientStats;

    // Configure adaptive rate limiting based on system health
    async fn set_system_health(&self, health: SystemHealth) -> f64;

    // Cancel a pending request if it hasn't been processed yet
    async fn cancel_request(&self, correlation_id: &str) -> bool;

    // Update client configuration
    async fn update_config(&self, config: ClientConfig) -> Result<(), ClientError>;

    // Pause/resume processing (for maintenance windows)
    async fn pause(&self, drain: bool) -> Result<(), ClientError>;
    async fn resume(&self) -> Result<(), ClientError>;

    // Forcibly clear circuit breakers (emergency use only)
    async fn reset_circuit_breakers(&self) -> usize;
}

#[derive(Debug)]
struct Bucket {
    pub rate_limit: AtomicUsize,
    pub rate_limit_window_ms: AtomicUsize,
    bucket: parking_lot::RwLock<Vec<Instant>>,
}

impl Bucket {
    pub fn get_token(&self) -> Result<(), ApiError> {
        let now = Instant::now();
        let limit = self.rate_limit.load(Ordering::SeqCst);
        let window_ms = self.rate_limit_window_ms.load(Ordering::SeqCst);

        // Clean up old requests beyond the window
        let window_duration = Duration::from_millis(window_ms as u64);
        self.bucket
            .write()
            .retain(|timestamp| now.duration_since(*timestamp) < window_duration);

        // Check if we've hit the rate limit
        if self.bucket.read().len() >= limit - 5 {
            return Err(ApiError::RateLimitExceeded(
                format!(
                    "Rate limit of {} requests per {}ms exceeded",
                    limit, window_ms
                ),
                limit,
                window_ms,
            ));
        }

        Ok(())
    }
}

impl Default for Bucket {
    fn default() -> Self {
        Self {
            rate_limit: AtomicUsize::new(10000),
            rate_limit_window_ms: AtomicUsize::new(10000000),
            bucket: parking_lot::RwLock::new(vec![]),
        }
    }
}

type RetryCount = u32;
type PqMapK = (Instant, RequestPriority, RetryCount, String);
type PqMapV = (SearchRequest, Sender<SearchResponse>);

// Booking API client to implement
#[derive(Default)]
pub struct BookingApiClient {
    // TODO: Add appropriate fields here
    // You'll likely need:
    // - Rate limiters (token bucket or leaky bucket)
    // - Priority queues for different request types
    // - Circuit breakers for downstream dependencies
    // - Request tracking for telemetry
    // - Connection pools
    // - Retry mechanisms with backoff and jitter
    search_pq: Arc<RwLock<BTreeMap<PqMapK, PqMapV>>>,
    book_pq: Arc<RwLock<BTreeMap<(Duration, usize), Vec<BookingRequest>>>>,
    config: ClientConfig,
    retry_config: RetryConfig,
    cb_config: CircuitBreakerConfig,
    stats: ClientStats,
    t_bucket: Bucket,
}

impl BookingApiClient {
    async fn start_search_queue(
        &self,
        tx: mpsc::Sender<(PqMapK, PqMapV)>,
        search_c_b: Arc<RwLock<CircuitBreaker>>,
    ) {
        loop {
            let is_cb_closed = search_c_b.write().await.should_allow_call();

            self.stats
                .circuit_breaker_open
                .swap(!is_cb_closed, Ordering::SeqCst);

            if self.t_bucket.get_token().is_ok() && is_cb_closed {
                let top = self.search_pq.write().await.pop_first();
                if let Some((k, req)) = top {
                    self.t_bucket.bucket.write().push(Instant::now());
                    let res = tx.send((k.clone(), req)).await;
                    res.unwrap();
                } else {
                    let _ = tokio::time::sleep(Duration::from_millis(200));
                }
            } else {
                let is_cb_closed = search_c_b.write().await.should_allow_call();
                self.stats
                    .circuit_breaker_open
                    .swap(!is_cb_closed, Ordering::SeqCst);
                let _ = tokio::time::sleep(Duration::from_secs(2));
            }
        }
    }

    async fn start_request_hadler(
        &self,
        server: Arc<MockServer>,
        search_c_b: Arc<RwLock<CircuitBreaker>>,
        mut rx: mpsc::Receiver<(PqMapK, PqMapV)>,
    ) {
        while let Some(((_, _, retry_count, _), (req, tx))) = rx.recv().await {
            // call to search API with circuit breaker
            if retry_count > 0 {
                // println!("retry cvount: {retry_count}");
                self.stats.requests_retried.fetch_add(1, Ordering::SeqCst);
            }

            let res = server.handle_search(req.clone()).await;

            match res {
                Ok(res) => {
                    let _ = tx.send(res);
                    self.stats.requests_succeeded.fetch_add(1, Ordering::SeqCst);
                    search_c_b.write().await.success();
                }
                Err(ApiError::RateLimitExceeded(msg, limit, ws_limit)) => {
                    println!("Error: {msg:?}");
                    self.stats.requests_failed.fetch_add(1, Ordering::SeqCst);

                    self.t_bucket.rate_limit.swap(limit, Ordering::SeqCst);
                    self.t_bucket
                        .rate_limit_window_ms
                        .swap(ws_limit + 100, Ordering::SeqCst);
                    self.stats
                        .current_rate_limit
                        .swap(limit as u32, Ordering::SeqCst);
                    self.stats.requests_throttled.fetch_add(1, Ordering::SeqCst);

                    // if fails end this back to the PQ with added jitter
                    if self.retry_config.max_retries > retry_count {
                        let backoff =
                            BookingApiClient::calculate_backoff(retry_count, &self.retry_config);

                        // re-insert the request into PQ
                        self.search_pq.write().await.insert(
                            (
                                Instant::now() + backoff,
                                req.priority,
                                retry_count + 1,
                                req.idempotency_key.clone().unwrap_or_default(),
                            ),
                            (req.clone(), tx),
                        );
                    } else {
                        self.stats.requests_preempted.fetch_add(1, Ordering::SeqCst);
                    }
                }
                _ => {
                    // TODO: Keep this below code in serparete func
                    search_c_b.write().await.fail();
                    self.stats.requests_failed.fetch_add(1, Ordering::SeqCst);
                    // if fails send this back to the PQ with added jitter
                    if self.retry_config.max_retries > retry_count {
                        let backoff =
                            BookingApiClient::calculate_backoff(retry_count, &self.retry_config);

                        // re-insert the request into PQ
                        self.search_pq.write().await.insert(
                            (
                                Instant::now() + backoff,
                                req.priority,
                                retry_count + 1,
                                req.idempotency_key.clone().unwrap_or_default(),
                            ),
                            (req.clone(), tx),
                        );
                    } else {
                        self.stats.requests_preempted.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        }
    }

    pub async fn start(
        my_client: Arc<BookingApiClient>,
        server: Arc<MockServer>,
        my_circuit_breaker: Arc<RwLock<CircuitBreaker>>,
    ) {
        let (tx, rx) = mpsc::channel::<(PqMapK, PqMapV)>(1000);

        let client = my_client.clone();

        let circuit_breaker = my_circuit_breaker.clone();

        // Starts a task to pop new SearchRequest from BTreeMap indexed based on jitter and prority
        let _ = tokio::spawn(async move {
            client.start_search_queue(tx, circuit_breaker).await;
        });

        // Starts a task to get request PQueue.
        // Check with CB if we can call this API and update th CB accordingly.
        // If fails: update the CB and retry with added jitter and move to next req
        // If success: just update the CB stats and move to next req
        let client = my_client.clone();
        let circuit_breaker = my_circuit_breaker.clone();

        let _ = tokio::spawn(async move {
            client
                .start_request_hadler(server, circuit_breaker, rx)
                .await
        });
    }
}

#[async_trait]
impl ApiClient for BookingApiClient {
    async fn search(&self, request: SearchRequest) -> Result<SearchResponse, ApiError> {
        // TODO: Implement with:
        //
        // - Rate limiting using token bucket algorithm
        // - Priority-based queueing
        // - Circuit breaker pattern
        // - Retry with exponential backoff and jitter
        let (tx, rx) = oneshot::channel::<SearchResponse>();
        let insert = self.search_pq.write().await.insert(
            (
                request.delay.unwrap_or_else(|| Instant::now()),
                request.priority,
                0,
                request.idempotency_key.clone().unwrap_or_default(),
            ),
            (request, tx),
        );

        if insert.is_some() {
            self.stats.requests_sent.fetch_add(1, Ordering::SeqCst);
        }

        tokio::spawn(async move {
            match rx.await {
                Ok(res) => {
                    println!("Response: {res:?}");
                    Ok(res)
                }
                Err(e) => {
                    println!("Error: {e:?}");
                    Err(ApiError::Other("NEoor in rcver ".to_string()))
                }
            }
        });

        // - Detailed telemetry collection
        // - Adaptive throttling based on system health
        Err(ApiError::Other("Not implemented".to_string()))
    }

    async fn book(&self, _request: BookingRequest) -> Result<BookingResponse, ApiError> {
        // TODO: Implement with higher priority than search requests
        // Bookings should be able to preempt search requests when needed
        Err(ApiError::Other("Not implemented".to_string()))
    }

    fn stats(&self) -> ClientStats {
        // TODO: Implement comprehensive statistics
        ClientStats::default()
    }

    async fn set_system_health(&self, health: SystemHealth) -> f64 {
        // TODO: Implement adaptive rate limiting based on system health
        // - Healthy: 100% of configured rate
        // - Degraded: 60% of configured rate
        // - Unhealthy: 20% of configured rate
        match health {
            SystemHealth::Healthy => 1.0,
            SystemHealth::Degraded => 0.6,
            SystemHealth::Unhealthy => 0.2,
        }
    }

    async fn cancel_request(&self, correlation_id: &str) -> bool {
        // TODO: Implement request cancellation
        false
    }

    async fn update_config(&self, _config: ClientConfig) -> Result<(), ClientError> {
        // TODO: Implement dynamic configuration updates

        Err(ClientError::ConfigError("Not implemented".to_string()))
    }

    async fn pause(&self, _drain: bool) -> Result<(), ClientError> {
        // TODO: Implement graceful pause
        Err(ClientError::ConfigError("Not implemented".to_string()))
    }

    async fn resume(&self) -> Result<(), ClientError> {
        // TODO: Implement resume
        Err(ClientError::ConfigError("Not implemented".to_string()))
    }

    async fn reset_circuit_breakers(&self) -> usize {
        // TODO: Implement circuit breaker reset
        0
    }
}

impl BookingApiClient {
    // Create a new client with the given configuration
    pub async fn new(config: ClientConfig) -> Result<Self, ClientError> {
        // TODO: Implement proper initialization of all components:
        // - Token bucket rate limiters
        // - Priority queues
        // - Circuit breakers
        // - Connection pools
        // - Metrics collection
        let client = Self::default();
        Ok(Self::default())
    }

    // Helper to calculate exponential backoff with jitter
    pub fn calculate_backoff(retry_attempt: u32, config: &RetryConfig) -> Duration {
        let base_backoff_ms = (config.initial_backoff_ms as f64
            * config.backoff_multiplier.powf(retry_attempt as f64))
        .min(config.max_backoff_ms as f64);

        // Apply jitter to prevent thundering herd
        let jitter = rand::random::<f64>() * config.jitter_factor * base_backoff_ms;
        let backoff_ms = base_backoff_ms * (1.0 - config.jitter_factor / 2.0) + jitter;

        Duration::from_millis(backoff_ms as u64)
    }
}

// Enhanced mock server for testing (you can modify or extend this)
// #[cfg(test)]
pub mod mock_server {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Instant;
    use tokio::sync::Mutex;

    #[derive(Debug, Clone, Copy)]
    pub enum ServerMode {
        Normal,
        Degraded,
        Overloaded,
        PartialOutage,
        CompleteOutage,
    }

    pub struct MockServer {
        mode: std::sync::atomic::AtomicU8,
        request_count: AtomicUsize,
        search_responses: Mutex<HashMap<String, SearchResponse>>,
        booking_responses: Mutex<HashMap<String, BookingResponse>>,
        fail_next_requests: AtomicUsize,
        delay_ms: AtomicUsize,
        rate_limit: AtomicUsize,
        rate_limit_window_ms: AtomicUsize,
        recent_requests: Mutex<Vec<(Instant, String)>>,
        dropped_request_count: AtomicUsize,
    }

    impl MockServer {
        pub fn new() -> Self {
            Self {
                mode: std::sync::atomic::AtomicU8::new(0), // Normal mode
                request_count: AtomicUsize::new(0),
                search_responses: Mutex::new(HashMap::new()),
                booking_responses: Mutex::new(HashMap::new()),
                fail_next_requests: AtomicUsize::new(0),
                delay_ms: AtomicUsize::new(0),
                rate_limit: AtomicUsize::new(1), // Default: 100 requests per window
                rate_limit_window_ms: AtomicUsize::new(1000), // Default: 1-second window
                recent_requests: Mutex::new(Vec::new()),
                dropped_request_count: AtomicUsize::new(0),
            }
        }

        pub fn set_mode(&self, mode: ServerMode) {
            let mode_value = match mode {
                ServerMode::Normal => 0,
                ServerMode::Degraded => 1,
                ServerMode::Overloaded => 2,
                ServerMode::PartialOutage => 3,
                ServerMode::CompleteOutage => 4,
            };
            self.mode.store(mode_value, Ordering::SeqCst);
        }

        pub fn set_delay(&self, delay_ms: usize) {
            self.delay_ms.store(delay_ms, Ordering::SeqCst);
        }

        pub fn set_rate_limit(&self, limit: usize, window_ms: usize) {
            self.rate_limit.store(limit, Ordering::SeqCst);
            self.rate_limit_window_ms.store(window_ms, Ordering::SeqCst);
        }

        pub fn fail_next_requests(&self, count: usize) {
            self.fail_next_requests.store(count, Ordering::SeqCst);
        }

        pub async fn add_search_response(&self, hotel_id: &str, response: SearchResponse) {
            let mut responses = self.search_responses.lock().await;
            responses.insert(hotel_id.to_string(), response);
        }

        pub async fn add_booking_response(&self, hotel_id: &str, response: BookingResponse) {
            let mut responses = self.booking_responses.lock().await;
            responses.insert(hotel_id.to_string(), response);
        }

        // Enhanced implementation - check rate limits, simulate failures based on mode
        pub async fn handle_search(
            &self,
            request: SearchRequest,
        ) -> Result<SearchResponse, ApiError> {
            // return Ok(SearchResponse::default());
            self.request_count.fetch_add(1, Ordering::SeqCst);

            // Check server mode
            let mode = self.mode.load(Ordering::SeqCst);
            match mode {
                4 => {
                    // Complete outage
                    return Err(ApiError::NetworkError("Service unavailable".to_string()));
                }
                3 => {
                    // Partial outage - 50% chance of failure
                    if rand::random::<f32>() < 0.5 {
                        return Err(ApiError::ApiResponseError {
                            status_code: 503,
                            message: "Service temporarily unavailable".to_string(),
                            is_retryable: true,
                        });
                    }
                }
                2 => {
                    // Partial outage - 20% chance of failure
                    if rand::random::<f32>() < 0.2 {
                        return Err(ApiError::ApiResponseError {
                            status_code: 503,
                            message: "Service temporarily unavailable".to_string(),
                            is_retryable: true,
                        });
                    }
                }
                1 => {
                    // Partial outage - 10% chance of failure
                    if rand::random::<f32>() < 0.1 {
                        return Err(ApiError::ApiResponseError {
                            status_code: 503,
                            message: "Service temporarily unavailable".to_string(),
                            is_retryable: true,
                        });
                    }
                }
                _ => {}
            }

            // Apply rate limiting
            let now = Instant::now();
            let limit = self.rate_limit.load(Ordering::SeqCst);
            let window_ms = self.rate_limit_window_ms.load(Ordering::SeqCst);

            let mut recent = self.recent_requests.lock().await;

            // Clean up old requests beyond the window
            let window_duration = Duration::from_millis(window_ms as u64);
            recent.retain(|(timestamp, _)| now.duration_since(*timestamp) < window_duration);

            // Check if we've hit the rate limit
            if recent.len() >= limit {
                self.dropped_request_count.fetch_add(1, Ordering::SeqCst);
                return Err(ApiError::RateLimitExceeded(
                    format!(
                        "R: {} Rate limit of {} requests per {}ms exceeded ",
                        request.idempotency_key.unwrap_or_default(),
                        limit,
                        window_ms
                    ),
                    limit,
                    window_ms,
                ));
            }

            // Track this request
            recent.push((now, request.context.correlation_id.clone()));

            // Simulate delay
            let delay = self.delay_ms.load(Ordering::SeqCst);
            if delay > 0 {
                // Add jitter for realism
                let jitter = if mode > 0 {
                    rand::random::<usize>() % delay
                } else {
                    0
                };
                tokio::time::sleep(Duration::from_millis((delay + jitter) as u64)).await;
            }

            // Simulate failures
            let fail_count = self.fail_next_requests.load(Ordering::SeqCst);
            if fail_count > 0 {
                self.fail_next_requests
                    .store(fail_count - 1, Ordering::SeqCst);
                return Err(ApiError::ApiResponseError {
                    status_code: 500,
                    message: "Internal Server Error".to_string(),
                    is_retryable: true,
                });
            }

            // Return mock response
            let responses = self.search_responses.lock().await;
            if let Some(hotel_id) = request.hotel_ids.first() {
                if let Some(response) = responses.get(hotel_id) {
                    let mut response = response.clone();
                    response.rate_limit_remaining = Some((limit - recent.len()) as u32);
                    return Ok(response);
                }
            }

            // Default response
            Ok(SearchResponse {
                search_id: format!(
                    "search-{}",
                    request
                        .idempotency_key
                        .unwrap_or_else(|| rand::random::<u32>().to_string())
                ),
                results: vec![],
                rate_limit_remaining: Some((limit - recent.len()) as u32),
                processing_time_ms: delay as u64,
            })
        }

        // Similar to handle_search but for booking
        pub async fn handle_booking(
            &self,
            request: BookingRequest,
        ) -> Result<BookingResponse, ApiError> {
            self.request_count.fetch_add(1, Ordering::SeqCst);

            // Prioritize bookings - they bypass rate limits but still affected by outages
            let mode = self.mode.load(Ordering::SeqCst);
            if mode == 4 {
                // Complete outage
                return Err(ApiError::NetworkError("Service unavailable".to_string()));
            }

            // Apply delay based on server mode
            let delay = self.delay_ms.load(Ordering::SeqCst);
            if delay > 0 {
                let actual_delay = match mode {
                    0 => delay,
                    1 => delay * 2, // Degraded adds 2x delay
                    2 => delay * 3, // Overloaded adds 3x delay
                    _ => delay * 5, // Partial outage adds 5x delay
                };
                tokio::time::sleep(Duration::from_millis(actual_delay as u64)).await;
            }

            // Simulate failures based on mode
            let fail_probability = match mode {
                0 => 0.0, // Normal: no random failures
                1 => 0.1, // Degraded: 10% failure
                2 => 0.3, // Overloaded: 30% failure
                _ => 0.5, // Partial outage: 50% failure
            };

            if rand::random::<f64>() < fail_probability {
                return Err(ApiError::ApiResponseError {
                    status_code: 500,
                    message: "Internal Server Error".to_string(),
                    is_retryable: true,
                });
            }

            // Return mock response
            let responses = self.booking_responses.lock().await;
            if let Some(response) = responses.get(&request.hotel_id) {
                return Ok(response.clone());
            }

            // Default response
            Ok(BookingResponse {
                booking_id: format!("booking-{}", rand::random::<u32>()),
                status: "confirmed".to_string(),
                confirmation_code: Some(format!("CONF{}", rand::random::<u16>())),
                rate_limit_remaining: None, // Bookings don't count against rate limit
                processing_time_ms: delay as u64,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mock_server::{MockServer, ServerMode};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_adaptive_rate_limiting() {
        // TODO: Implement this test
        // - Create a mock server that simulates different health states
        let server = Arc::new(MockServer::new());
        // - Configure client with appropriate settings
        let config = ClientConfig::default();
        let client = Arc::new(BookingApiClient::new(config).await.unwrap());
        let circuit_breaker = Arc::new(RwLock::new(CircuitBreaker::new(
            client.cb_config.failure_threshold,
            client.cb_config.success_threshold,
            client.cb_config.reset_timeout_ms,
        )));

        let mc = client.clone();

        let old_limit = client.t_bucket.rate_limit.load(Ordering::SeqCst);
        let old_limit_ws = client.t_bucket.rate_limit_window_ms.load(Ordering::SeqCst);
        assert_eq!(old_limit, 10000);
        assert_eq!(old_limit_ws, 10000000);

        tokio::spawn(async move {
            let _ =
                BookingApiClient::start(mc.clone(), server.clone(), circuit_breaker.clone()).await;
        });

        let mc = client.clone();
        tokio::spawn(async move {
            for _ in 0..10 {
                let _ = mc.search(SearchRequest::default()).await;
            }
        });

        // wait before server shutdowns
        tokio::time::sleep(Duration::from_secs(10)).await;

        let new_limit = client.t_bucket.rate_limit.load(Ordering::SeqCst);
        let new_limit_ws = client.t_bucket.rate_limit_window_ms.load(Ordering::SeqCst);

        assert_eq!(new_limit, 1);
        assert_eq!(new_limit_ws, 1000);
        // - Test that client adapts rate limits based on server health
        // - Verify statistics reflect the adaptations
    }

    #[tokio::test]
    async fn test_circuit_breaker() {
        // TODO: Implement this test
        // - Create a mock server that consistently fails
        // - Configure client with circuit breaker settings
        // - Send requests until circuit breaker trips
        // - Verify that subsequent requests fail fast with CircuitBreakerOpen
        // - Wait for reset timeout
        // - Verify circuit breaker allows half-open testing
        let server = Arc::new(MockServer::new());
        server.set_rate_limit(1000, 100000);
        let config = ClientConfig::default();
        let client = Arc::new(BookingApiClient::new(config).await.unwrap());
        let circuit_breaker = Arc::new(RwLock::new(CircuitBreaker::new(
            client.cb_config.failure_threshold,
            client.cb_config.success_threshold,
            client.cb_config.reset_timeout_ms,
        )));

        let mc = client.clone();
        let cb = circuit_breaker.clone();
        let sc = server.clone();

        tokio::spawn(async move {
            let _ = BookingApiClient::start(mc.clone(), sc, cb).await;
        });

        let mc = client.clone();

        server.set_mode(ServerMode::CompleteOutage);
        tokio::spawn(async move {
            for _ in 0..6 {
                let _ = mc.search(SearchRequest::default()).await;
            }
        });
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(false, circuit_breaker.read().await.is_closed());

        server.set_mode(ServerMode::Normal);
        tokio::time::sleep(Duration::from_millis(client.cb_config.reset_timeout_ms)).await;

        // check if CB again reset to closed
        assert_eq!(true, circuit_breaker.read().await.is_closed());

        // wait before server shutdowns
        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    #[tokio::test]
    async fn test_prioritization_and_preemption() {
        // TODO: Implement this test
        // - Create a mock server with limited concurrency
        // - Send many low priority requests to saturate the server
        // - Then send high priority requests
        // - Verify high priority requests complete before low priority ones
        // - Verify some low priority requests were preempted

        let server = Arc::new(MockServer::new());
        server.set_rate_limit(20, 1000);

        let config = ClientConfig::default();
        let client = Arc::new(BookingApiClient::new(config).await.unwrap());

        let circuit_breaker = Arc::new(RwLock::new(CircuitBreaker::new(
            client.cb_config.failure_threshold,
            client.cb_config.success_threshold,
            client.cb_config.reset_timeout_ms,
        )));

        let mc = client.clone();
        let cb = circuit_breaker.clone();
        let sc = server.clone();

        tokio::spawn(async move {
            let _ = BookingApiClient::start(mc.clone(), sc, cb).await;
        });

        let mc = client.clone();

        server.set_delay(2000);
        let req_delay = tokio::time::Instant::now();
        tokio::spawn(async move {
            for i in 0..10 {
                // High Priority Request
                let req1 = SearchRequest {
                    priority: RequestPriority::High,
                    idempotency_key: Some((i + 10).to_string()),
                    delay: Some(req_delay),
                    ..Default::default()
                };

                // Low Priority Request
                let req2 = SearchRequest {
                    priority: RequestPriority::Low,
                    idempotency_key: Some((i).to_string()),
                    delay: Some(req_delay),
                    ..Default::default()
                };

                let _ = mc.search(req2).await;
                let _ = mc.search(req1).await;
            }
        });

        // wait before server shutdowns
        tokio::time::sleep(Duration::from_secs(20)).await;
    }

    #[tokio::test]
    async fn test_retry_with_backoff() {
        // TODO: Implement this test
        // - Create a mock server that fails a specific number of times
        // - Send a request that triggers retries
        // - Measure time between retries to verify backoff
        // - Verify request eventually succeeds
        // - Check that retry statistics are updated

        let server = Arc::new(MockServer::new());
        server.set_rate_limit(20, 1000);

        let config = ClientConfig::default();
        let client = Arc::new(BookingApiClient::new(config).await.unwrap());

        let circuit_breaker = Arc::new(RwLock::new(CircuitBreaker::new(
            client.cb_config.failure_threshold,
            client.cb_config.success_threshold,
            client.cb_config.reset_timeout_ms,
        )));

        let mc = client.clone();
        let cb = circuit_breaker.clone();

        let sc = server.clone();
        sc.fail_next_requests(5);

        tokio::spawn(async move {
            let _ = BookingApiClient::start(mc.clone(), sc, cb).await;
        });

        let mc = client.clone();

        tokio::spawn(async move {
            for i in 0..10 {
                // High Priority Request
                let req1 = SearchRequest {
                    idempotency_key: Some((i).to_string()),
                    ..Default::default()
                };

                let _ = mc.search(req1).await;
            }
        });

        // wait before server shutdowns
        tokio::time::sleep(Duration::from_secs(10)).await;

        // check if exactly 5 req got retry once
        assert_eq!(client.stats.requests_retried.load(Ordering::SeqCst), 5);
    }

    #[tokio::test]
    async fn test_extreme_load_handling() {
        // TODO: Implement this test
        // - Create a client with limited capacity
        // - Simultaneously send hundreds or thousands of requests
        // - Verify client maintains stability
        // - Check that low priority requests are rejected when overloaded
        // - Verify high priority requests still get through
        // - Check statistics for throughput and latency
        let server = Arc::new(MockServer::new());
        server.set_rate_limit(200, 1000);

        let config = ClientConfig::default();
        let client = Arc::new(BookingApiClient::new(config).await.unwrap());

        let circuit_breaker = Arc::new(RwLock::new(CircuitBreaker::new(
            client.cb_config.failure_threshold,
            client.cb_config.success_threshold,
            client.cb_config.reset_timeout_ms,
        )));

        let mc = client.clone();
        let cb = circuit_breaker.clone();

        let sc = server.clone();
        sc.set_mode(ServerMode::Degraded);

        tokio::spawn(async move {
            let _ = BookingApiClient::start(mc.clone(), sc, cb).await;
        });

        let mc = client.clone();

        tokio::spawn(async move {
            for i in 0..5000 {
                // High Priority Request
                let req1 = SearchRequest {
                    idempotency_key: Some((i).to_string()),
                    ..Default::default()
                };

                let _ = mc.search(req1).await;
            }
        });

        // wait before server shutdowns
        tokio::time::sleep(Duration::from_secs(500)).await;

        // check if exactly 5 req got retry once
        // assert_eq!(client.stats.requests_retried.load(Ordering::SeqCst), 5);
        // assert_eq!(client.stats.requests_failed.load(Ordering::SeqCst), 5);
        println!(
            "fail count: {}, retry count: {}, pre count: {}",
            client.stats.requests_failed.load(Ordering::SeqCst),
            client.stats.requests_retried.load(Ordering::SeqCst),
            client.stats.requests_preempted.load(Ordering::SeqCst)
        );
    }
}
