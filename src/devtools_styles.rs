//! One response page and one in-flight style request, scoped to existing inspector authority.
use std::time::{Duration, Instant};
use wega::engine_protocol::{
    DocumentContext,
    inspector::{dom::InspectorNodeId, styles::*, *},
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    #[default]
    Inline,
    Computed,
    Sheets,
    Rules,
    Declarations,
    Matched,
}
#[derive(Debug)]
pub(crate) struct StyleView {
    pub enabled: bool,
    pub mode: Mode,
    pub page: Option<StylePage>,
    pub focus: Option<RuleRef>,
    pub notice: String,
    pub selection: Option<InspectorNodeId>,
    binding: Option<(TargetId, InspectorSessionId, DocumentContext, u64)>,
    pending: Option<(StyleQuery, Instant)>,
    offset: u32,
    due: Option<Instant>,
}
impl Default for StyleView {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: Mode::Inline,
            page: None,
            focus: None,
            notice: String::new(),
            selection: None,
            binding: None,
            pending: None,
            offset: 0,
            due: None,
        }
    }
}
impl StyleView {
    pub fn reset(&mut self) {
        let mode = self.mode;
        let enabled = self.enabled;
        *self = Self::default();
        self.enabled = enabled;
        self.mode = match mode {
            Mode::Rules | Mode::Declarations => Mode::Sheets,
            _ => mode,
        };
    }
    pub fn select(&mut self, node: Option<InspectorNodeId>) {
        if self.selection != node {
            self.reset();
            self.selection = node;
        }
    }
    pub fn change_mode(&mut self, mode: Mode) {
        if self.mode != mode {
            self.mode = mode;
            self.refresh();
        }
    }
    pub fn refresh(&mut self) {
        self.page = None;
        self.offset = 0;
        self.due = None;
        self.notice.clear();
    }
    pub fn show_rules(&mut self, rule: RuleRef, declarations: bool) {
        self.mode = if declarations {
            Mode::Declarations
        } else {
            Mode::Rules
        };
        self.focus = Some(rule);
        self.refresh();
    }
    pub fn first(&mut self) {
        self.offset = 0;
        self.page = None;
        self.due = None;
    }
    pub fn more(&mut self) {
        if let Some(next) = self.page.as_ref().and_then(|p| p.next) {
            self.offset = next;
            self.page = None;
            self.due = None;
        }
    }
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    fn wants(&self, q: &StyleQuery) -> bool {
        q.offset() == self.offset
            && match (&self.mode, q) {
                (Mode::Inline, StyleQuery::Inline { .. })
                | (Mode::Computed, StyleQuery::Computed { .. })
                | (Mode::Sheets, StyleQuery::Sheets { .. })
                | (Mode::Matched, StyleQuery::Matched { .. }) => true,
                (Mode::Rules, StyleQuery::Rules { parent, .. }) => {
                    self.focus.as_ref() == Some(parent)
                }
                (Mode::Declarations, StyleQuery::Declarations { rule, .. }) => {
                    self.focus.as_ref() == Some(rule)
                }
                _ => false,
            }
    }
    pub fn prepare(
        &mut self,
        now: Instant,
        target: TargetId,
        session: InspectorSessionId,
        document: DocumentContext,
        epoch: u64,
        selection: Option<InspectorNodeId>,
    ) -> Option<StyleQuery> {
        self.select(selection);
        let binding = (target, session, document, epoch);
        if self.binding != Some(binding) {
            let selection = self.selection;
            self.reset();
            self.selection = selection;
            self.binding = Some(binding);
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|(_, at)| now.saturating_duration_since(*at) > Duration::from_millis(2100))
        {
            self.pending = None;
            self.page = None;
            self.notice = "Style request timed out".into();
            self.due = Some(now + Duration::from_secs(1));
        }
        if !self.enabled || self.pending.is_some() || self.due.is_some_and(|at| now < at) {
            return None;
        }
        if self.page.is_some() && !matches!(self.mode, Mode::Computed) {
            return None;
        }
        let offset = self.offset;
        Some(match self.mode {
            Mode::Sheets => StyleQuery::Sheets { offset },
            Mode::Rules => StyleQuery::Rules {
                parent: self.focus.clone()?,
                offset,
            },
            Mode::Declarations => StyleQuery::Declarations {
                rule: self.focus.clone()?,
                offset,
            },
            Mode::Inline => StyleQuery::Inline {
                node: self.selection?,
                offset,
            },
            Mode::Computed => StyleQuery::Computed {
                node: self.selection?,
                offset,
            },
            Mode::Matched => StyleQuery::Matched {
                node: self.selection?,
            },
        })
    }
    pub fn begin(&mut self, query: StyleQuery, now: Instant) {
        self.pending = Some((query, now));
        self.due = Some(now + Duration::from_secs(1));
    }
    pub fn complete(&mut self, result: Result<InspectorResponse, InspectorError>) {
        let Some((query, _)) = self.pending.take() else {
            return;
        };
        match result {
            Ok(InspectorResponse::Style(page))
                if self.wants(&page.query)
                    && page.is_bounded()
                    && page.query == query
                    && self.binding
                        == Some((
                            page.scope.target,
                            page.scope.session,
                            page.scope.document,
                            page.scope.epoch,
                        ))
                    && page.query.node().is_none_or(|n| Some(n) == self.selection) =>
            {
                self.page = Some(*page);
                self.notice.clear();
            }
            Err(error) => {
                self.page = None;
                self.notice = error.message;
                if matches!(
                    error.code,
                    InspectorErrorCode::StaleMutation
                        | InspectorErrorCode::StaleDocument
                        | InspectorErrorCode::Unauthorized
                ) {
                    self.focus = None;
                    if matches!(self.mode, Mode::Rules | Mode::Declarations) {
                        self.mode = Mode::Sheets;
                        self.offset = 0;
                    }
                }
            }
            _ => {
                self.page = None;
                self.notice = "Stale or mismatched style response discarded".into();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wega::engine_protocol::{
        DocumentId, DocumentRevision,
        identity::{FrameId, ProcessSessionId, TabId},
    };
    fn scope() -> StyleScope {
        StyleScope {
            target: TargetId {
                tab: TabId(1),
                frame: FrameId(1),
                process: ProcessSessionId([1; 16]),
            },
            session: InspectorSessionId(1),
            document: DocumentContext {
                document: DocumentId(1),
                revision: DocumentRevision(1),
            },
            epoch: 1,
            generation: 1,
        }
    }
    fn prepared() -> (StyleView, StyleQuery, StyleScope, Instant) {
        let scope = scope();
        let now = Instant::now();
        let mut v = StyleView {
            mode: Mode::Sheets,
            ..Default::default()
        };
        let q = v
            .prepare(
                now,
                scope.target,
                scope.session,
                scope.document,
                scope.epoch,
                None,
            )
            .unwrap();
        v.begin(q.clone(), now);
        (v, q, scope, now)
    }
    fn page(scope: StyleScope, query: StyleQuery) -> InspectorResponse {
        InspectorResponse::Style(Box::new(StylePage {
            scope,
            query,
            availability: StyleAvailability::Available,
            layout_pending: false,
            total: 0,
            next: None,
            items: vec![],
            truncated: false,
            note: "Author sheets only".into(),
        }))
    }
    #[test]
    fn caches_one_page_and_one_pending_request_with_timeout() {
        let (mut v, q, s, now) = prepared();
        assert!(
            v.prepare(now, s.target, s.session, s.document, s.epoch, None)
                .is_none()
        );
        v.complete(Ok(page(s, q)));
        assert!(v.page.is_some());
        assert!(
            v.prepare(
                now + Duration::from_secs(3),
                s.target,
                s.session,
                s.document,
                s.epoch,
                None
            )
            .is_none()
        );
        v.refresh();
        let q = v
            .prepare(now, s.target, s.session, s.document, s.epoch, None)
            .unwrap();
        v.begin(q, now);
        assert!(
            v.prepare(
                now + Duration::from_secs(3),
                s.target,
                s.session,
                s.document,
                s.epoch,
                None
            )
            .is_none()
        );
        assert!(!v.pending());
    }
    #[test]
    fn mode_changes_and_foreign_responses_do_not_render_previous_query() {
        let (mut v, q, s, _) = prepared();
        v.change_mode(Mode::Computed);
        v.complete(Ok(page(s, q)));
        assert!(v.page.is_none());
        let (mut v, q, mut s, _) = prepared();
        s.target.process = ProcessSessionId([9; 16]);
        v.complete(Ok(page(s, q)));
        assert!(v.page.is_none());
    }
    #[test]
    fn navigation_session_process_selection_and_epoch_clear_cached_rules() {
        for dimension in 0..5 {
            let (mut v, q, s, now) = prepared();
            v.complete(Ok(page(s, q)));
            let (mut t, mut session, mut doc, mut epoch, mut node) =
                (s.target, s.session, s.document, s.epoch, None);
            match dimension {
                0 => t.process = ProcessSessionId([2; 16]),
                1 => session = InspectorSessionId(2),
                2 => doc.revision = DocumentRevision(2),
                3 => epoch += 1,
                _ => {
                    node = Some(InspectorNodeId {
                        document: doc,
                        key: 9,
                    })
                }
            };
            let _ = v.prepare(now, t, session, doc, epoch, node);
            assert!(v.page.is_none());
            assert!(v.focus.is_none());
        }
    }
    #[test]
    fn unsupported_and_not_ready_have_visible_labels() {
        let (mut v, q, s, _) = prepared();
        v.complete(Ok(page(s, q)));
        v.page.as_mut().unwrap().availability = StyleAvailability::Unsupported;
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            crate::devtools_styles_ui::draw(ui, &mut v)
        });
        let text = out
            .shapes
            .iter()
            .filter_map(|s| {
                if let egui::Shape::Text(t) = &s.shape {
                    Some(t.galley.job.text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("Unsupported") && text.contains("not an empty"));
        out.textures_delta.clear();
    }
}
