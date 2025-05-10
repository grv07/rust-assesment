# B2B Holidays Technical Assessment

This repository contains the starter template for the B2B Holidays Senior Rust Developer technical assessment. It provides the structure and interfaces for the three assessment tasks.

## Assessment Overview

This assessment evaluates your proficiency with Rust and related technologies used in our high-performance travel technology platform. You should expect to spend approximately 8-12 hours to complete all parts thoroughly. Focus on code quality, performance, and proper error handling.

The assessment consists of three parts with graduated difficulty levels:

1. **Hotel Availability Cache** (Moderate Difficulty): Implement a thread-safe, high-performance cache for hotel availability data
2. **XML Processing** (Entry Level): Implement an efficient XML processor for hotel search responses
3. **Rate-Limited API Client** (Advanced): Implement an async rate-limited client for a hotel booking API that handles extreme traffic

This graduated approach reflects our system architecture where some components face more demanding requirements than others.

## Getting Started

1. Clone this repository
2. Install Rust (if not already installed) using [rustup](https://rustup.rs/)
3. Build the project to verify everything works:
   ```
   cargo build
   ```
4. Run the tests to make sure the environment is set up correctly:
   ```
   cargo test
   ```

## Project Structure

- `src/part1_cache.rs`: Hotel Availability Cache implementation task
- `src/part2_xml.rs`: XML Processing implementation task
- `src/part3_api.rs`: Rate-Limited API Client implementation task
- `benches/cache_benchmark.rs`: Benchmark template for the cache implementation

## Implementation Instructions

For each part of the assessment:

1. Read the trait/struct definitions and comments carefully
2. Implement the required functionality
3. Add tests to verify your implementation
4. Ensure your code is well-documented with comments

### Time Allocation Guidelines

To help you plan your time effectively, here's a suggested breakdown:

**Part 1: Hotel Availability Cache (Moderate Difficulty)**
- Implementation: 2-3 hours
- Testing: 1-2 hours

**Part 2: XML Processing (Entry Level)**
- Implementation: 1-2 hours
- Testing: 1 hour

**Part 3: Rate-Limited API Client (Advanced)**
- Implementation: 3-5 hours
- Testing: 1-2 hours

You may choose to focus more deeply on certain parts based on your strengths and interests. A basic implementation that meets core requirements for all parts is preferable to an exhaustive implementation of just one part.

### Part 1: Hotel Availability Cache (Moderate Difficulty)

Implement the `AvailabilityCache` trait in `part1_cache.rs`. This component serves as the middleware between our high-traffic customer-facing API and our supplier systems.

Your implementation should:
- Be thread-safe for concurrent access with minimal lock contention
- Enforce memory usage limits with customizable eviction policies
- Implement TTL-based expiration with efficient cleanup
- Prioritize frequently accessed items using a carefully chosen algorithm
- Track and report detailed cache statistics
- Handle cache stampedes for popular items
- Optimize for read-heavy workloads with infrequent writes

Your tests should cover:
- Basic operations (store, get)
- Concurrent access behavior with high contention scenarios
- Expiration handling with clock manipulation
- Memory limit enforcement with large items
- Cache eviction strategies under pressure
- Performance characteristics under various access patterns

Once implemented, update the benchmark in `benches/cache_benchmark.rs` to measure your cache's performance under realistic load.

### Part 2: XML Processing and JSON Conversion (Entry Level)

Implement the `HotelSearchProcessor` in `src/part2_xml.rs` to efficiently process data from hotel suppliers. This is our data transformation layer between supplier systems and the cache.

Your implementation should:

1. Convert supplier JSON responses to XML format for client consumption
2. Parse and extract key information from XML hotel search responses
3. Filter hotel options based on various criteria
4. Handle moderate-sized documents efficiently

Your tests should cover:
- JSON to XML conversion accuracy
- XML parsing and extraction
- Filter criteria application
- Basic performance with standard documents

Sample files are provided for testing:
- `samples/supplier_response.json`: Sample JSON response from suppliers
- `samples/hotel_search_response.xml`: Sample XML response for clients
- `samples/hotel_search_request.xml`: Sample XML request

This reflects the actual data flow in our system where supplier data comes in JSON format and is converted to XML for customer responses.

### Part 3: Rate-Limited API Client (Advanced Difficulty)

Implement the `BookingApiClient` in `src/part3_api.rs`. This component is our customer-facing API that must handle extreme traffic while maintaining high reliability and performance.

Your implementation should:
- Enforce sophisticated rate limiting with token bucket or leaky bucket algorithms
- Implement adaptive rate limiting based on service health
- Manage concurrent requests with priority queuing and fairness guarantees
- Implement circuit breaking for failing dependencies
- Use timeouts with exponential backoff strategies
- Prioritize booking requests over search requests with preemption
- Handle retries for transient failures with jitter
- Maintain detailed telemetry for monitoring and diagnostics
- Optimize for high throughput while maintaining low latency
- Handle graceful degradation under extreme load

Your tests should cover:
- Rate limiting enforcement under burst and sustained load
- Concurrent request handling with varied request types
- Timeout behavior with slow dependencies
- Request prioritization with resource contention
- Retry logic for different failure scenarios
- Circuit breaker behavior with unhealthy dependencies
- Performance characteristics under simulated production load

## Testing Guidelines

For each component, focus on testing:
1. **Correctness**: Does your implementation behave as specified?
2. **Edge Cases**: How does it handle unusual inputs or conditions?
3. **Performance**: Is it efficient for the expected load?
4. **Concurrency**: Does it handle parallel access correctly?
5. **Resilience**: How does it behave under failure conditions?

The template includes example test stubs to get you started. You don't need exhaustive tests for every possible scenario - focus on key behaviors and requirements.

### Test Coverage Expectations

For each component, we expect:

- **Minimum Coverage**: Aim for at least 80% test coverage for your implementation
- **Unit Tests**: All public methods should have corresponding unit tests
- **Error Handling**: Tests should verify proper error handling for invalid inputs and failure scenarios

**Part 1 (Cache):**
- Tests must verify thread safety under concurrent access
- Tests must confirm memory limits are enforced correctly
- Tests must validate that eviction policies work as expected
- At least one performance test measuring throughput under load

**Part 2 (XML Processing):**
- Tests must verify correct parsing of all sample files
- Tests must validate filtering logic with various criteria
- Tests should check error handling for malformed XML
- Tests should verify JSON to XML conversion accuracy

**Part 3 (API Client):**
- Tests must verify rate limiting under various load conditions
- Tests must confirm circuit breaking behavior works correctly
- Tests must validate request prioritization and preemption
- Tests should verify retry logic with backoff and jitter
- Tests should check adaptive rate limiting based on system health

Provide appropriate mocks or test doubles where needed to isolate your tests from external dependencies.

## Running Benchmarks

To run the cache benchmark (once implemented):

```
cargo bench
```

## Submission Guidelines

1. Create a private GitHub repository with your solution
2. Ensure your code compiles without warnings
3. Include a README.md with:
   - Setup and running instructions
   - Brief explanation of your approach for each part
   - Any assumptions or trade-offs you made
   - Ideas for further improvements (if you had more time)
4. Email the URL of your GitHub repository to: `it@b2bholidays.com` with the subject line "Rust Developer Assessment Submission"
   - If your repository is private, please make sure to add `B2B-HOLIDAYS` as a collaborator with read access

## Evaluation Criteria

Your solution will be evaluated based on:

1. **Correctness**: Does it meet the requirements and handle edge cases?
2. **Performance**: Is it optimized for throughput and resource usage?
3. **Code Quality**: Is it well-structured, documented, and maintainable?
4. **Rust Proficiency**: Does it use appropriate Rust idioms and patterns?
5. **Error Handling**: Does it handle errors gracefully and informatively?
6. **Concurrency**: Does it handle concurrent operations safely and efficiently?
7. **System Design**: Do your solutions demonstrate understanding of distributed systems principles?

Good luck! We're looking for clean, efficient solutions that demonstrate your understanding of high-performance Rust development.
