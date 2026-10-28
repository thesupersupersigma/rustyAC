// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `GPUProfiler` (0x30, `GraphicsManager::gpuProfiler`): timestamp queries around the render
//! passes of one frame in 32, read back without waiting 32 frames later, for the developers'
//! GPU profiler app. Off until that app is first drawn, then on for good.

use crate::kgl::{Kgl, KglQuery};

/// `GPUProfiler::QueryData` (0x38).
pub struct QueryData {
    pub query: Option<KglQuery>,
    pub last_result: f64,
    pub last_ts: f64,
    pub name: &'static str,
}

pub struct GpuProfiler {
    pub queries: Vec<QueryData>,
    pub wait_count: i32,
    pub frame_latency: i32,
    pub is_active: bool,
    /// `qDisjoint` and `DisjointData.Frequency`, kgl's globals
    disjoint: Option<KglQuery>,
    frequency: u64,
}

impl GpuProfiler {
    /// `GPUProfiler::GPUProfiler` 0x140227f70 (`kglInitGPUQuerySupport`: the disjoint query).
    pub fn new(kgl: &Kgl) -> GpuProfiler {
        GpuProfiler { queries: Vec::new(), wait_count: 0, frame_latency: 30, is_active: false, disjoint: kgl.new_query(false), frequency: 0 }
    }

    /// The gate of `beginFrame`, `triggerQuery`: the measured frame.
    fn measuring(&self) -> bool {
        self.is_active && self.frame_latency < self.wait_count
    }

    /// `GPUProfiler::beginFrame` 0x140228350.
    pub fn begin_frame(&mut self, kgl: &Kgl) {
        if self.measuring() {
            self.fetch_results(kgl);
            if let Some(q) = &self.disjoint {
                kgl.query_begin(q);
            }
            self.trigger_query(kgl, "BEGIN_FRAME");
        }
    }

    /// `GPUProfiler::endFrame` 0x1402283f0 (its `30 < waitCount` is `frameLatency`'s 30 written out).
    pub fn end_frame(&mut self, kgl: &Kgl) {
        if self.is_active {
            if 30 < self.wait_count {
                self.trigger_query(kgl, "PRESENT");
                if let Some(q) = &self.disjoint {
                    kgl.query_end(q);
                }
                self.wait_count = 0;
            } else {
                self.wait_count += 1;
            }
        }
    }

    /// `GPUProfiler::fetchResults` 0x1402284a0: each row's time since the row before (the first
    /// row's: its own timestamp, as in the game).
    fn fetch_results(&mut self, kgl: &Kgl) {
        if let Some(q) = &self.disjoint {
            kgl.query_frequency(q, &mut self.frequency);
        }
        let mut previous = 0.0;
        for row in &mut self.queries {
            let ts = row.query.as_ref().map_or(0.0, |q| kgl.query_timestamp(q, self.frequency));
            row.last_ts = ts;
            row.last_result = ts - previous;
            previous = ts;
        }
    }

    /// `GPUProfiler::triggerQuery` 0x1402288b0: a pass's timestamp, its query made the first time
    /// the name is seen in a measured frame (so the rows keep that order).
    pub fn trigger_query(&mut self, kgl: &Kgl, name: &'static str) {
        if !self.measuring() {
            return;
        }
        let index = match self.queries.iter().position(|q| q.name == name) {
            Some(index) => index,
            None => {
                self.queries.push(QueryData { query: kgl.new_query(true), last_result: 0.0, last_ts: 0.0, name });
                self.queries.len() - 1
            }
        };
        if let Some(q) = &self.queries[index].query {
            kgl.query_end(q);
        }
    }
}
