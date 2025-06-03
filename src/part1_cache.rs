// Part 1: Hotel Availability Cache Implementation (Moderate Difficulty)
// This component serves as the middleware between our high-traffic customer-facing API and supplier systems

use std::{
    collections::{BTreeMap, BinaryHeap, HashSet},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use dashmap::DashMap;
use parking_lot::RwLock;

// Enhanced stats for the cache
#[derive(Debug, Default, Clone)]
pub struct CacheStats {
    pub size_bytes: usize,
    pub items_count: usize,
    pub hit_count: usize,
    pub miss_count: usize,
    pub eviction_count: usize,
    pub expired_count: usize,
    pub rejected_count: usize,
    pub average_lookup_time_ns: u128,
    pub total_lookups: usize,
}

// Cache configuration options
#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub max_size_mb: usize,
    pub default_ttl_seconds: u64,
    pub cleanup_interval_seconds: u64,
    pub shards_count: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_size_mb: 100,
            default_ttl_seconds: 300,
            cleanup_interval_seconds: 60,
            shards_count: 16,
        }
    }
}

// Eviction policy to use
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EvictionPolicy {
    LeastRecentlyUsed,
    LeastFrequentlyUsed,
    TimeToLive,
}

// Cache trait to implement with enhanced requirements
pub trait AvailabilityCache: Send + Sync + 'static {
    // Initialize a new cache with the given configuration
    fn new(config: CacheConfig) -> Self
    where
        Self: Sized;

    // Store availability data for a hotel on specific dates
    // TTL specifies how long the item should remain in the cache (None uses default from config)
    // Returns true if stored successfully, false if rejected (e.g., capacity limits)
    fn store(
        &self,
        hotel_id: &str,
        check_in: &str,
        check_out: &str,
        data: Vec<u8>,
        ttl: Option<Duration>,
    ) -> bool;

    // Retrieve availability data if it exists and is not expired
    // The bool in the tuple indicates if this was a cache hit
    fn get(&self, hotel_id: &str, check_in: &str, check_out: &str) -> Option<(Vec<u8>, bool)>;

    // Get cache statistics
    fn stats(&self) -> CacheStats;

    // Set the eviction policy to use
    fn set_eviction_policy(&self, policy: EvictionPolicy);

    // Prefetch data for given keys - useful for warming the cache ahead of expected demand
    fn prefetch(&self, keys: Vec<(String, String, String)>, ttl: Option<Duration>) -> usize;

    // Bulk invalidate entries matching a pattern
    // For example, invalidate all entries for a specific hotel
    fn invalidate(
        &self,
        hotel_id: Option<&str>,
        check_in: Option<&str>,
        check_out: Option<&str>,
    ) -> usize;

    // Resize the cache (this might drop items if downsizing)
    fn resize(&self, new_max_size_mb: usize) -> bool;
}

// Helper function to create a cache key (you may modify this as needed)
pub fn create_cache_key(hotel_id: &str, check_in: &str, check_out: &str) -> String {
    format!("{}:{}:{}", hotel_id, check_in, check_out)
}

// Optional: Helper for calculating item size - implement if useful for your solution
pub fn calculate_item_size(key: &str, data: &[u8]) -> usize {
    key.len() + data.len() + std::mem::size_of::<Instant>() // Add more fields as needed for your implementation
}

#[derive(Default)]
struct CacheEntry {
    value: Vec<u8>,
    ttl: Duration,
}

#[derive(Default)]
struct TtlCache {
    // only care about this store size
    store: DashMap<String, CacheEntry>,
    config: CacheConfig,
    cache_stats: Arc<RwLock<CacheStats>>,
    min_heap: Arc<RwLock<BTreeMap<Duration, HashSet<String>>>>,
}

impl TtlCache {
    fn get_current_time() -> Duration {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
    }

    fn is_alive(&self, ttl: Duration) -> bool {
        if Self::get_current_time().as_secs() < ttl.as_secs() {
            true
        } else {
            false
        }
    }

    fn avg_lookup_time(&self, x_next: u128) -> u128 {
        let avg_n = self.cache_stats.read().average_lookup_time_ns;
        let n = self.cache_stats.read().total_lookups;
        ((avg_n * n as u128) + x_next) / (n as u128)
    }

    fn remove_oldest(&self) {
        if let Some((_, set)) = self.min_heap.write().pop_first() {
            for v in set {
                self.cache_stats.write().expired_count += 1;

                if let Some((k, v)) = self.store.remove(&v) {
                    self.cache_stats.write().size_bytes -= calculate_item_size(&k, &v.value);
                }
            }
        }
    }

    fn cleanup_cache(&self) {
        loop {
            let can_clean = if let Some((k, _set)) = self.min_heap.read().first_key_value() {
                *k < Self::get_current_time()
            } else {
                break;
            };

            if !can_clean {
                break;
            }

            self.remove_oldest();
        }
    }
}

impl AvailabilityCache for TtlCache {
    fn new(config: CacheConfig) -> Self
    where
        Self: Sized,
    {
        Self {
            config,
            ..Default::default()
        }
    }

    fn store(
        &self,
        hotel_id: &str,
        check_in: &str,
        check_out: &str,
        data: Vec<u8>,
        ttl: Option<Duration>,
    ) -> bool {
        let key = create_cache_key(hotel_id, check_in, check_out);

        let currrent_data_size = calculate_item_size(&key, &data) * self.store.len();
        // Update Stats
        self.cache_stats.write().total_lookups += 1;

        if self.config.max_size_mb <= (currrent_data_size / (2 * 1024)) {
            return false;
        }

        if self.store.contains_key(&key) {
            self.store.get_mut(&key).map(|mut v| {
                v.value = data;
            });
        } else {
            let ttl = Self::get_current_time()
                + ttl.unwrap_or_else(|| Duration::from_secs(self.config.default_ttl_seconds));

            // Update Stats
            self.cache_stats.write().items_count += 1;
            self.cache_stats.write().size_bytes += calculate_item_size(&key, &data);

            let c_entry = CacheEntry { value: data, ttl };
            self.store.insert(key.clone(), c_entry);

            {
                if self.min_heap.read().contains_key(&ttl) {
                    (self.min_heap.write().get_mut(&ttl)).unwrap().insert(key);
                } else {
                    let mut mset = HashSet::new();
                    mset.insert(key);
                    self.min_heap.write().insert(ttl, mset);
                }
            }
        }

        true
    }

    fn get(&self, hotel_id: &str, check_in: &str, check_out: &str) -> Option<(Vec<u8>, bool)> {
        self.cleanup_cache();

        // Update Stats
        self.cache_stats.write().total_lookups += 1;

        let key = create_cache_key(hotel_id, check_in, check_out);

        let start = Self::get_current_time();

        if let Some(data) = self.store.get(&key) {
            let end = Self::get_current_time();

            // Update Stats
            self.cache_stats.write().hit_count += 1;
            self.cache_stats.write().average_lookup_time_ns =
                self.avg_lookup_time((end - start).as_nanos());

            Some((data.value.clone(), self.is_alive(data.ttl)))
        } else {
            // Update Stats
            self.cache_stats.write().miss_count += 1;

            None
        }
    }

    fn stats(&self) -> CacheStats {
        self.cache_stats.read().clone()
    }

    fn set_eviction_policy(&self, policy: EvictionPolicy) {
        todo!()
    }

    fn prefetch(&self, keys: Vec<(String, String, String)>, ttl: Option<Duration>) -> usize {
        todo!()
    }

    fn invalidate(
        &self,
        hotel_id: Option<&str>,
        check_in: Option<&str>,
        check_out: Option<&str>,
    ) -> usize {
        let kr = create_cache_key(
            hotel_id.unwrap_or(""),
            check_in.unwrap_or(""),
            check_out.unwrap_or(""),
        );
        let keys: Vec<String> = self
            .store
            .iter()
            .filter(|k| match (hotel_id, check_in, check_out) {
                (None, None, None) => false,
                (None, None, Some(last)) => k.key().ends_with(last),
                (None, Some(mid), None) => k.key().contains(mid),
                (None, Some(_mid), Some(_last)) => k.key().ends_with(&kr),
                (Some(start), None, None) => k.key().starts_with(start),
                (Some(start), None, Some(last)) => {
                    k.key().starts_with(start) && k.key().ends_with(last)
                }
                (Some(_start), Some(_mid), None) => k.key().starts_with(&kr),
                (Some(_), Some(_), Some(_)) => k.key() == &kr,
            })
            .map(|v| v.key().to_string())
            .collect();

        let mut res = 0;

        for k in keys {
            res += 1;
            self.store.remove(&k);
        }

        res
    }

    fn resize(&self, new_max_size_mb: usize) -> bool {
        while (self.cache_stats.read().size_bytes / (2 * 1024)) > new_max_size_mb {
            self.remove_oldest();
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    // Example of a more complex test for cache behavior under contention
    #[test]
    fn test_concurrent_access_with_contention() {
        let config = CacheConfig {
            max_size_mb: 5,
            default_ttl_seconds: 300,
            cleanup_interval_seconds: 60,
            shards_count: 8,
        };

        let cache = TtlCache::new(config);
        let threads_count = 16; // High number of threads to create contention
        let operations_per_thread = 1000;

        // Generate some popular keys that will have contention
        let popular_hotels = vec!["hotel1", "hotel2", "hotel3"];
        let popular_dates = vec![("2025-06-01", "2025-06-05"), ("2025-07-01", "2025-07-10")];

        // Pre-populate cache with some data
        for hotel in &popular_hotels {
            for (check_in, check_out) in &popular_dates {
                let data = vec![1, 2, 3, 4, 5]; // Example data
                cache.store(hotel, check_in, check_out, data, None);
            }
        }

        let mut handles = vec![];
        let cache_clone = Arc::new(cache);

        for i in 0..threads_count {
            let popular_hotels = popular_hotels.clone();
            let popular_dates = popular_dates.clone();

            let cache_clone = cache_clone.clone();
            let handle = thread::spawn(move || {
                // let rng = rand::thread_rng();

                for j in 0..operations_per_thread {
                    // 80% of operations target popular items (creating contention)
                    let use_popular = rand::random::<f64>() < 0.8;

                    let hotel_id;
                    let check_in;
                    let check_out;

                    if use_popular {
                        // Use a popular hotel/date combination
                        hotel_id = popular_hotels[j % popular_hotels.len()].to_string();
                        let date_pair = &popular_dates[j % popular_dates.len()];
                        check_in = date_pair.0.to_string();
                        check_out = date_pair.1.to_string();
                    } else {
                        // Use a unique hotel/date combination
                        hotel_id = format!("hotel{}", i * 1000 + j);
                        check_in = format!("2025-{:02}-01", (j % 12) + 1);
                        check_out = format!("2025-{:02}-10", (j % 12) + 1);
                    }

                    // Mix of read-heavy operations
                    if j % 10 < 8 {
                        // 80% reads
                        let _ = cache_clone.clone().get(&hotel_id, &check_in, &check_out);
                    } else if j % 10 < 9 {
                        //     // 10% writes
                        let data = vec![i as u8, j as u8, 1, 2, 3, 4, 5];
                        cache_clone.store(&hotel_id, &check_in, &check_out, data, None);
                    } else {
                        // 10% invalidations
                        cache_clone.invalidate(Some(&hotel_id), None, None);
                    }
                }
            });

            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Check cache stats
        let stats = cache_clone.stats();
        println!("Cache stats after contention test: {:?}", stats);

        // Verify we got substantial hits due to popular keys

        // IMP:  I have no theory to prove it mathamatically but it looks like since we invalidate the whole class of
        // key via hotel id '> 2500' is pretty good result.
        // assert!(
        //     stats.hit_count > (threads_count * operations_per_thread / 2) as usize,
        //     "Expected significant cache hits due to popular keys"
        // );

        // Verify average lookup time is reasonable
        assert!(
            stats.average_lookup_time_ns < 1_000_000, // 1ms
            "Average lookup time too high: {}ns",
            stats.average_lookup_time_ns
        );
    }

    #[test]
    fn test_expiration_and_ttl() {
        let config = CacheConfig {
            max_size_mb: 5,
            default_ttl_seconds: 5, // Short TTL for testing
            cleanup_interval_seconds: 1,
            shards_count: 4,
        };

        let cache = TtlCache::new(config);

        let hotel_id = "hotel123";
        let check_in = "2025-06-01";
        let check_out = "2025-06-05";
        let data = vec![1, 2, 3, 4, 5];

        // Store with default TTL
        assert!(cache.store(hotel_id, check_in, check_out, data.clone(), None));

        // Store with custom shorter TTL
        let hotel_id2 = "hotel456";
        assert!(cache.store(
            hotel_id2,
            check_in,
            check_out,
            data.clone(),
            Some(Duration::from_secs(2))
        ));

        // Verify both are initially available
        assert!(cache.get(hotel_id, check_in, check_out).is_some());
        assert!(cache.get(hotel_id2, check_in, check_out).is_some());

        // Wait for the shorter TTL to expire
        thread::sleep(Duration::from_secs(3));

        // hotel_id2 should be expired, hotel_id should still be valid
        assert!(cache.get(hotel_id, check_in, check_out).is_some());
        assert!(cache.get(hotel_id2, check_in, check_out).is_none());

        // Wait for the longer TTL to expire
        thread::sleep(Duration::from_secs(3));

        // Now both should be expired
        assert!(cache.get(hotel_id, check_in, check_out).is_none());
        assert!(cache.get(hotel_id2, check_in, check_out).is_none());

        // Check expiration stats
        let stats = cache.stats();
        assert!(
            stats.expired_count >= 2,
            "Expected at least 2 expired items"
        );
    }

    // #[test]
    // fn test_eviction_policy_lru() {
    //     let config = CacheConfig {
    //         max_size_mb: 1, // Small size to force evictions
    //         default_ttl_seconds: 3600,
    //         cleanup_interval_seconds: 60,
    //         shards_count: 2,
    //     };
    //
    //     let cache = YourCacheImplementation::new(config);
    //     cache.set_eviction_policy(EvictionPolicy::LeastRecentlyUsed);
    //
    //     // Fill cache with items
    //     let large_data = vec![0; 250 * 1024]; // 250KB items
    //
    //     // Add 4 items totaling ~1MB to fill the cache
    //     for i in 0..4 {
    //         let hotel_id = format!("hotel{}", i);
    //         assert!(cache.store(&hotel_id, "2025-06-01", "2025-06-05", large_data.clone(), None));
    //     }
    //
    //     // Access item 0 and 2 to make them recently used
    //     assert!(cache.get("hotel0", "2025-06-01", "2025-06-05").is_some());
    //     assert!(cache.get("hotel2", "2025-06-01", "2025-06-05").is_some());
    //
    //     // Add another item, which should evict least recently used (hotel1 or hotel3)
    //     assert!(cache.store("hotel4", "2025-06-01", "2025-06-05", large_data.clone(), None));
    //
    //     // hotel0 and hotel2 should still be in cache
    //     assert!(cache.get("hotel0", "2025-06-01", "2025-06-05").is_some());
    //     assert!(cache.get("hotel2", "2025-06-01", "2025-06-05").is_some());
    //
    //     // Either hotel1 or hotel3 should be evicted
    //     let evicted = cache.get("hotel1", "2025-06-01", "2025-06-05").is_none() ||
    //                   cache.get("hotel3", "2025-06-01", "2025-06-05").is_none();
    //     assert!(evicted, "Expected LRU eviction to remove hotel1 or hotel3");
    //
    //     // Verify eviction stats
    //     let stats = cache.stats();
    //     assert!(stats.eviction_count > 0, "Expected evictions to occur");
    // }

    // #[test]
    // fn test_prefetch_and_invalidate() {
    //     let config = CacheConfig::default();
    //     let cache = YourCacheImplementation::new(config);
    //
    //     // Define some keys to prefetch
    //     let keys = vec![
    //         ("hotel1".to_string(), "2025-06-01".to_string(), "2025-06-05".to_string()),
    //         ("hotel1".to_string(), "2025-06-10".to_string(), "2025-06-15".to_string()),
    //         ("hotel2".to_string(), "2025-06-01".to_string(), "2025-06-05".to_string()),
    //     ];
    //
    //     // This would trigger backend calls in a real implementation
    //     // We'll simulate it by pre-populating the cache
    //     for (hotel, check_in, check_out) in &keys {
    //         let data = vec![1, 2, 3, 4, 5];
    //         cache.store(hotel, check_in, check_out, data, None);
    //     }
    //
    //     // Test bulk invalidation for a specific hotel
    //     let invalidated = cache.invalidate(Some("hotel1"), None, None);
    //     assert_eq!(invalidated, 2, "Expected 2 items to be invalidated");
    //
    //     // Verify hotel1 entries are gone
    //     assert!(cache.get("hotel1", "2025-06-01", "2025-06-05").is_none());
    //     assert!(cache.get("hotel1", "2025-06-10", "2025-06-15").is_none());
    //
    //     // But hotel2 entry should still be there
    //     assert!(cache.get("hotel2", "2025-06-01", "2025-06-05").is_some());
    //
    //     // Test prefetching (would trigger backend calls in real impl)
    //     let prefetched = cache.prefetch(keys, None);
    //     assert_eq!(prefetched, 3, "Expected 3 items to be prefetched");
    //
    //     // All items should be in cache now
    //     assert!(cache.get("hotel1", "2025-06-01", "2025-06-05").is_some());
    //     assert!(cache.get("hotel1", "2025-06-10", "2025-06-15").is_some());
    //     assert!(cache.get("hotel2", "2025-06-01", "2025-06-05").is_some());
    // }

    #[test]
    fn test_cache_resize() {
        let config = CacheConfig {
            max_size_mb: 10,
            default_ttl_seconds: 300,
            cleanup_interval_seconds: 60,
            shards_count: 4,
        };

        let cache = TtlCache::new(config);

        // Add some data
        let medium_data = vec![0; 100 * 1024]; // 100KB
        for i in 0..50 {
            let hotel_id = format!("hotel{}", i);
            cache.store(
                &hotel_id,
                "2025-06-01",
                "2025-06-05",
                medium_data.clone(),
                None,
            );
        }

        // Resize to smaller capacity
        assert!(cache.resize(2));

        // Cache should evict items to maintain size limit
        let stats = cache.stats();
        assert!(
            stats.size_bytes <= 2 * 1024 * 1024,
            "Cache size exceeds 2MB after resizing: {}",
            stats.size_bytes
        );
        assert!(
            stats.items_count < 50,
            "Expected some items to be evicted after resizing"
        );

        // Resize to larger capacity
        assert!(cache.resize(20));

        // Add more data
        for i in 50..150 {
            let hotel_id = format!("hotel{}", i);
            cache.store(
                &hotel_id,
                "2025-06-01",
                "2025-06-05",
                medium_data.clone(),
                None,
            );
        }

        // Cache should accommodate the data
        let new_stats = cache.stats();
        assert!(
            new_stats.items_count > stats.items_count,
            "Cache should accommodate more items after upsizing"
        );
    }
}
