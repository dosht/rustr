//! YouTube caption cues to lines (TRX-038, `resolve.ts:106-113`): one line per
//! cue, `start` and `start + duration` floored, no speaker, no language, no
//! translation status, no speaker-split flag.

use media_ports::CaptionCue;

use super::NewLine;
use super::time::seconds_time;

pub fn normalize(cues: &[CaptionCue]) -> Vec<NewLine> {
    cues.iter()
        .map(|cue| NewLine {
            start_time: seconds_time(cue.start),
            end_time: seconds_time(cue.start + cue.duration),
            line: cue.text.clone(),
            speaker: None,
            corrected_text: None,
            split_on_speaker_change: None,
            language: None,
            translation_status: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_floor_start_and_end() {
        let lines = normalize(&[CaptionCue { text: "hello".into(), start: 1.9, duration: 2.3 }]);
        assert_eq!(lines[0].start_time, "00:00:01");
        assert_eq!(lines[0].end_time, "00:00:04");
        assert_eq!(lines[0].speaker, None);
        assert_eq!(lines[0].split_on_speaker_change, None);
    }
}
