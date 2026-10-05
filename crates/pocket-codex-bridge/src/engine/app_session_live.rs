//! A bounded live tail, independent of paginated history and disk retention.

use std::collections::{BTreeMap, HashMap};

use super::ThreadItem;

const MAX_ITEMS: usize = 1024;
const MAX_THREADS: usize = 8;
const MAX_BYTES: usize = 32 * 1024 * 1024;

type Key = (String, String);

struct Entry {
    item: ThreadItem,
    sequence: u64,
    bytes: usize,
}

pub(super) struct LiveTranscript {
    items: HashMap<Key, Entry>,
    order: BTreeMap<u64, Key>,
    threads: HashMap<String, u64>,
    clock: u64,
    bytes: usize,
    accept_unseen_deltas: bool,
}

impl Default for LiveTranscript {
    fn default() -> Self {
        Self {
            items: HashMap::new(),
            order: BTreeMap::new(),
            threads: HashMap::new(),
            clock: 0,
            bytes: 0,
            accept_unseen_deltas: true,
        }
    }
}

impl LiveTranscript {
    pub(super) fn remove(&mut self, thread: &str) {
        let keys: Vec<_> = self
            .items
            .keys()
            .filter(|(t, _)| t == thread)
            .cloned()
            .collect();
        for key in keys {
            self.remove_item(&key);
        }
        self.threads.remove(thread);
    }

    fn remove_item(&mut self, key: &Key) {
        if let Some(entry) = self.items.remove(key) {
            self.bytes = self.bytes.saturating_sub(entry.bytes);
            self.order.remove(&entry.sequence);
        }
    }

    fn touch(&mut self, thread: &str) {
        self.clock += 1;
        self.threads.insert(thread.into(), self.clock);
    }

    pub(super) fn upsert(&mut self, thread: &str, item: ThreadItem) {
        let key = (thread.to_owned(), item.id.clone());
        let bytes = item_bytes(&item) + 2 * (thread.len() + item.id.len());
        let sequence = self.items.get(&key).map(|e| e.sequence);
        self.remove_item(&key);
        if bytes > MAX_BYTES {
            self.accept_unseen_deltas = false;
            return;
        }
        self.touch(thread);
        let sequence = sequence.unwrap_or(self.clock);
        self.bytes += bytes;
        self.order.insert(sequence, key.clone());
        self.items.insert(key, Entry {
            item,
            sequence,
            bytes,
        });
        self.trim();
    }

    /// False permits seeding a delta-only stream before any eviction. After
    /// pressure, only full snapshots may seed missing items: a suffix must
    /// never masquerade as a complete prefix in resume or a durable
    /// checkpoint.
    pub(super) fn append(&mut self, thread: &str, id: &str, text: &str) -> bool {
        let key = (thread.to_owned(), id.to_owned());
        let Some(entry) = self.items.get_mut(&key) else {
            return !self.accept_unseen_deltas;
        };
        if entry.bytes.saturating_add(text.len()) > MAX_BYTES {
            self.remove_item(&key);
            self.accept_unseen_deltas = false;
            return true;
        }
        self.bytes -= entry.bytes;
        entry.item.text.push_str(text);
        entry.bytes = item_bytes(&entry.item) + 2 * (thread.len() + id.len());
        self.bytes += entry.bytes;
        self.touch(thread);
        self.trim();
        true
    }

    fn trim(&mut self) {
        while self.threads.len() > MAX_THREADS {
            let oldest = self
                .threads
                .iter()
                .min_by_key(|(_, at)| *at)
                .map(|(t, _)| t.clone());
            if let Some(thread) = oldest {
                self.remove(&thread);
                self.accept_unseen_deltas = false;
            }
        }
        while self.items.len() > MAX_ITEMS || self.bytes > MAX_BYTES {
            let Some((_, key)) = self.order.first_key_value() else { break };
            self.remove_item(&key.clone());
            self.accept_unseen_deltas = false;
        }
    }

    pub(super) fn tail(&self, thread: &str, limit: usize) -> Vec<ThreadItem> {
        let mut items: Vec<_> = self
            .order
            .values()
            .rev()
            .filter(|(t, _)| t == thread)
            .filter_map(|key| self.items.get(key))
            .take(limit)
            .map(|entry| entry.item.clone())
            .collect();
        items.reverse();
        items
    }
}

fn item_bytes(item: &ThreadItem) -> usize {
    std::mem::size_of::<Entry>()
        + item.id.capacity()
        + item.item_type.capacity()
        + item.title.capacity()
        + item.text.capacity()
        + item.turn_id.capacity()
        + item.questions_json.as_ref().map_or(0, String::capacity)
        + item.images.capacity() * std::mem::size_of::<String>()
        + item.images.iter().map(String::capacity).sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: String, text: String) -> ThreadItem {
        ThreadItem {
            id,
            text,
            item_type: "agentMessage".into(),
            title: String::new(),
            questions_json: None,
            images: Vec::new(),
            turn_id: "turn".into(),
            turn_completed_at: None,
            turn_duration_ms: None,
        }
    }

    #[test]
    fn eviction_bounds_items_threads_and_bytes_without_caching_suffixes() {
        let mut live = LiveTranscript::default();
        for i in 0..MAX_ITEMS + 10 {
            live.upsert("thread", item(i.to_string(), "prefix".into()));
        }
        assert_eq!(live.items.len(), MAX_ITEMS);
        assert!(live.append("thread", "0", "suffix"));
        assert!(!live.tail("thread", MAX_ITEMS).iter().any(|i| i.id == "0"));
        live.upsert("thread", item("0".into(), "complete snapshot".into()));
        assert_eq!(live.tail("thread", 1)[0].text, "complete snapshot");
        for i in 0..MAX_THREADS + 5 {
            live.upsert(&i.to_string(), item("one".into(), "text".into()));
        }
        assert_eq!(live.threads.len(), MAX_THREADS);
        live.upsert("large", item("large".into(), "x".repeat(MAX_BYTES + 1)));
        assert!(live.bytes <= MAX_BYTES);
        assert!(live.tail("large", 1).is_empty());
    }

    #[test]
    fn cumulative_bytes_and_delta_growth_stay_bounded() {
        let mut live = LiveTranscript::default();
        for i in 0..40 {
            live.upsert("t", item(i.to_string(), "x".repeat(1024 * 1024)));
        }
        assert!(live.bytes <= MAX_BYTES);
        assert!(live.items.len() < 40);
        assert!(live.append("t", "39", &"x".repeat(MAX_BYTES)));
        assert!(live.bytes <= MAX_BYTES);
        assert!(!live.tail("t", MAX_ITEMS).iter().any(|i| i.id == "39"));
        assert!(live.append("t", "39", "late suffix"));
        assert!(!live.tail("t", MAX_ITEMS).iter().any(|i| i.id == "39"));
    }

    #[test]
    fn growth_preserves_order_and_replaces_snapshots() {
        let mut live = LiveTranscript::default();
        live.upsert("t", item("a".into(), "hello".into()));
        live.upsert("t", item("b".into(), "next".into()));
        assert!(live.append("t", "a", " world"));
        assert_eq!(live.tail("t", 2)[0].text, "hello world");
        live.upsert("t", item("a".into(), "final".into()));
        assert_eq!(live.tail("t", 2)[0].text, "final");
        assert_eq!(live.bytes, live.items.values().map(|e| e.bytes).sum::<usize>());
        live.remove("t");
        assert_eq!(live.bytes, 0);
    }
}
