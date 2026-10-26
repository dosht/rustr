//! Which layout each part of the app renders. The source has three separate
//! rules, kept apart here so no shell holds a breakpoint or a flag:
//!
//! - the frame, home and job list: touch and width below 1024 with the
//!   `MOBILE_UX_JOBS` capability (`useDeviceType.ts`, `JobListContainer.tsx`,
//!   `home/[[...jobId]].tsx`);
//! - an uploaded file's transcript: width below 768 only (`useMobileView.ts`),
//!   so a narrow non-touch window shows the tabbed transcript inside the
//!   desktop frame; it also picks the AI auto-summary trigger tab;
//! - a YouTube transcript: touch only, any width (`Transcript.tsx:469`).
//!
//! The phone landscape notice is a shell mechanic, not a layout.

use serde::{Deserialize, Serialize};

use crate::flags::Capabilities;

pub const FRAME_BREAKPOINT: u32 = 1024;
pub const TRANSCRIPT_BREAKPOINT: u32 = 768;

/// What the shell measured: `ontouchstart` or `maxTouchPoints > 0`, and
/// `innerWidth`, sent at start and on change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Viewport {
    pub touch: bool,
    pub width: u32,
}

impl Default for Viewport {
    /// The source's first render before measuring: no touch, width 1024.
    fn default() -> Viewport {
        Viewport { touch: false, width: FRAME_BREAKPOINT }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Frame {
    #[default]
    Desktop,
    Phone,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum TranscriptLayout {
    /// Side by side panes.
    #[default]
    Split,
    /// One view at a time with a view switcher.
    Tabbed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum YouTubeLayout {
    #[default]
    Desktop,
    Touch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Layouts {
    pub frame: Frame,
    pub transcript: TranscriptLayout,
    pub youtube: YouTubeLayout,
    /// The phone AI views (`MOBILE_UX_AI`) inside a phone frame.
    pub phone_ai: bool,
}

pub fn layouts(viewport: Viewport, capabilities: &Capabilities) -> Layouts {
    let phone = viewport.touch && viewport.width < FRAME_BREAKPOINT && capabilities.mobile_ux_jobs;
    Layouts {
        frame: if phone { Frame::Phone } else { Frame::Desktop },
        transcript: if viewport.width < TRANSCRIPT_BREAKPOINT { TranscriptLayout::Tabbed } else { TranscriptLayout::Split },
        youtube: if viewport.touch { YouTubeLayout::Touch } else { YouTubeLayout::Desktop },
        phone_ai: phone && capabilities.mobile_ux_ai,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(jobs: bool, ai: bool) -> Capabilities {
        Capabilities { mobile_ux_jobs: jobs, mobile_ux_ai: ai, ..Capabilities::default() }
    }

    #[test]
    fn the_frame_is_phone_only_for_touch_below_1024_with_the_capability() {
        let on = caps(true, true);
        assert_eq!(layouts(Viewport { touch: true, width: 1023 }, &on).frame, Frame::Phone);
        assert_eq!(layouts(Viewport { touch: true, width: 1024 }, &on).frame, Frame::Desktop);
        assert_eq!(layouts(Viewport { touch: false, width: 400 }, &on).frame, Frame::Desktop);
        assert_eq!(layouts(Viewport { touch: true, width: 400 }, &caps(false, true)).frame, Frame::Desktop);
    }

    #[test]
    fn a_narrow_non_touch_window_tabs_the_transcript_inside_the_desktop_frame() {
        let l = layouts(Viewport { touch: false, width: 700 }, &caps(true, true));
        assert_eq!((l.frame, l.transcript, l.youtube, l.phone_ai), (Frame::Desktop, TranscriptLayout::Tabbed, YouTubeLayout::Desktop, false));
        assert_eq!(layouts(Viewport { touch: false, width: 768 }, &caps(true, true)).transcript, TranscriptLayout::Split);
    }

    #[test]
    fn youtube_follows_touch_at_any_width_and_phone_ai_needs_both_flags() {
        let wide_touch = layouts(Viewport { touch: true, width: 1400 }, &caps(true, true));
        assert_eq!((wide_touch.frame, wide_touch.youtube), (Frame::Desktop, YouTubeLayout::Touch));
        assert!(layouts(Viewport { touch: true, width: 500 }, &caps(true, true)).phone_ai);
        assert!(!layouts(Viewport { touch: true, width: 500 }, &caps(true, false)).phone_ai);
        assert_eq!(Layouts::default().frame, layouts(Viewport::default(), &Capabilities::default()).frame);
    }
}
