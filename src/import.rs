//! DB 取り込み用の計画データ（Phase 2）。

#[derive(Debug, Clone)]
pub struct PagePlan {
    pub file_name: String,
    pub seq: i64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub source: &'static str,
    pub prompt_hash: Option<String>,
    pub segment_index: Option<usize>,
    pub txt_status: &'static str,
}

#[derive(Debug, Clone)]
pub struct SegmentPlan {
    pub seq: i64,
    pub run_length: i64,
    pub pos: String,
    pub neg: String,
    pub prompt_hash: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct LeafPlan {
    pub root_path: String,
    pub name: String,
    pub parent_id: Option<i64>,
    pub imported_at: String,
    pub pages: Vec<PagePlan>,
    pub segments: Vec<SegmentPlan>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ImportCounts {
    pub gallery_count: u64,
    pub page_count: u64,
    pub segment_count: u64,
    pub unextracted_count: u64,
}

impl ImportCounts {
    pub fn add_leaf(&mut self, plan: &LeafPlan, new_gallery_row: bool) {
        if new_gallery_row {
            self.gallery_count += 1;
        }
        self.page_count += plan.pages.len() as u64;
        self.segment_count += plan.segments.len() as u64;
        self.unextracted_count += plan
            .pages
            .iter()
            .filter(|page| page.prompt_hash.is_none())
            .count() as u64;
    }

    pub fn add_container(&mut self) {
        self.gallery_count += 1;
    }
}

#[derive(Debug, Clone)]
pub struct SegmentRowInfo {
    pub id: i64,
    pub line_id: i64,
    pub seq: i64,
    pub run_length: i64,
}

#[derive(Debug, Clone)]
pub struct ImportedLeaf {
    pub gallery_id: i64,
    pub story_id: i64,
    pub segments: Vec<SegmentRowInfo>,
}
