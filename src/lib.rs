// Main library file for the travel tech assessment

// Export modules for each part of the assessment
pub mod part1_cache;
pub mod part2_xml;
// pub mod part3_api;

// Re-export key types for convenience
pub use part1_cache::{AvailabilityCache, CacheStats};
pub use part2_xml::{
    FilterCriteria, HotelOption, HotelSearchProcessor, ProcessedResponse, ProcessingError,
};
// pub use part3_api::{
//     ApiClient, ApiError, BookingApiClient, ClientConfig, ClientError, ClientStats,
// };
