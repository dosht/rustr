# P4 content review (p1R, read-only)

Scope: `transgate/core/src/ai/{analysis.rs, analysis/, prompts.rs, sanitize.rs}` against D1/D4/D5 and the read-only source in `/home/mu/Business/Transgate/frontend`. One bounded pass; nothing built, run or edited. Exact-byte E4 belongs to p24 and is not repeated here. Policy integration and the serialized snapshot cap belong to p1X and are not flagged.

## Findings

### 1. A schema-invalid or inapplicable section interrupts the whole analysis; the source keeps rendering (needs a decision)

`parser.rs:149-158,189-223` turns any section whose data fails the typed shape into `InvalidSection`, which fails `finish` and interrupts the generation. The source never does that:

- `analyze.ts:253-259` forwards every section whose `data || items || content` is not `undefined`, without validation.
- The renderers validate per item: `lib/ai/validation.ts` `validateArrayForRender` keeps valid items and drops invalid ones, and `validateForRender` hides an invalid object. Callers include `components/AIAnalysis/specialized/interview/SkillsAssessment.tsx:62`, `NotableResponses.tsx:71`, `CandidateAssessment.tsx:73` and `lecture/TopicsList.tsx`.
- A section the detected type has no renderer for is simply not shown.
- A section whose `data`, `items` and `content` are all null, false, 0, "" or absent yields `undefined` and is skipped (`analyze.ts:256-257`).

Counterexamples, each a well-formed JSON document that D1 does not name as an interruption cause:

- Meeting `actionItems` with one item `"status":"completed"`: the source shows the other items; Rust interrupts.
- Interview `skillsAssessment` with one `"rating":6`: the source drops that skill; Rust interrupts.
- `{"type":"assessment","data":{"position":null}}`: the source hides the assessment card; Rust interrupts.
- `{"type":"discussionPoints","data":null}` or a `"highlights"` section on a meeting: the source skips it; Rust interrupts.

D1 lists malformed or incomplete documents, conflicting duplicates, an unusable type or summary, and unsuccessful finishes. Schema-invalid content inside a valid document is not on that list. `tests.rs:90-103` (status `unknown`) and `tests.rs:126-141` (ratings 5.01 and 0, `position:null`, cross-type `callMetrics`) pin the interrupting behavior. The lead should either record this as a named correction or keep valid items and sections with a warning, matching the source's render-time filtering.

### 2. A usable detected type interrupts when its confidence is missing, non-numeric or above 100 (needs a decision)

`parser.rs:133-134` requires a finite confidence in 0..=100. The source uses `typeof parsed.confidence === 'number' ? parsed.confidence : 0` with no range check (`analyze.ts:247-248`). `TypeDetectionDocumentSchema` (`schemas.ts`, min 0, max 100) is not applied at runtime. `{"type":"detectedType","value":"meeting","confidence":"95"}` keeps a usable meeting type in the source and interrupts in Rust; `tests.rs:139-140` pins this for 101. D1's "unusable type" covers a missing or unknown value, not a valid value with a bad confidence.

### 3. The delimiter must be exactly `---` on its line (concrete framing risk)

`parser.rs:52-53,175-177` accepts only `---`, `---\r` or `---\r\n` as the entire line. The prompt says `"---" on its own line`, and the source splits on any `---` (`analyze.ts:231`). These all merge two documents and end in `MalformedDocument`:

- `---` followed by a trailing space or tab.
- A leading space before `---`.
- `---{"type":...}` with the next document on the same line.

A standalone line whose JS-trimmed content is `---`, still outside strings and containers, keeps the D1 framing correction and covers the first two cases. There is no regression for any of them.

### 4. A section before the detected type interrupts (low)

`parser.rs:158` returns `MissingDetectedType` for a non-summary section that arrives before `detectedType`. The source renders sections in any order. The prompt asks for the type second, so this is rare, but it is an undecided ordering rule. A summary before the type is already handled.

### 5. Source `String.replace` expands `$` patterns in the transcript (named correction missing)

`prompt-templates.ts:55-57` builds the analysis prompt with `template.user.replace('{{transcript}}', transcript)`. With a string replacement, JavaScript expands `$$`, `$&`, `` $` `` and `$'` inside the transcript text. A line `It costs $$5` reaches the provider as `It costs $5`; `$&` becomes `{{transcript}}`; `` $` `` inserts the whole prompt prefix. `prompts.rs:93-98` inserts the text literally, which is the right behavior but is not recorded as a correction. A `$&` and `$$` fixture would let p24's E4 show the difference explicitly.

### 6. The analysis token estimate does not match the source value (metadata, needs a decision)

`content.md` describes the analysis estimate as `round(UTF16(actual submitted canonical transcript)/4)`. The source computes it from `line.speaker_id || 'Speaker'` and `line.text || line.content || ''` (`analyze.ts:287-291`). The `transcripts` table has only `speaker` and `line` (`20251130113249_remote_schema.sql`, table `transcripts`). The source value is therefore `round(len(n × "Speaker: " joined by "\n") / 4) = round((10n - 1) / 4)` for n lines, independent of the content. That is a source bug, but the current text describes a different number without naming the correction. It flows into the stored `tokens_used` metadata.

## Missing regressions (only if the current behavior is kept)

- A delimiter line with trailing or leading whitespace, and `---` followed by a document on the same line.
- A transcript containing `$&` and `$$` in `analysis_messages`.
- `data:null` and `data:""` with no `items` or `content`.
- A detected type with a string confidence, and a section arriving before the detected type.

## Checked and consistent with the source

- Sanitation (`content-processor.ts:104-193`): script-block removal (same first-closing-tag reach), HTML tags, SQL words with ASCII boundaries and case folding, `javascript:`, `on\w+\s*=`, the ECMAScript `\s` set for collapsing and trimming, the order of the sensitive patterns, and the always-true API-key predicate. Output keeps script contents and SQL words, as the source does.
- `transcript_text` (`content-processor.ts:44-57`): untrimmed speaker, trimmed line, blank lines dropped, joined with one space.
- Context, input-security, raw-question and optional-question checks (`content-processor.ts:85-99,155-164`, `chat.ts:46-71`, `chat-validator.ts:46-74`): the same UTF-16 limits, order and messages; the validator context is the raw `transcriptLinesToText`.
- Analysis system and user templates, and the generated six-schema documentation: equal to `prompt-templates.ts:93-146` and to what `schema-to-prompt.ts` `generateAnalysisTypePromptSection` and `formatObjectForPrompt` produce from `schemas.ts`, including defaults, `string[]` versus `Array`, number bounds and enum spellings.
- The analysis transcript uses `speaker || 'Speaker'` and the raw line, joined with newlines, and ignores language (`ai-service-manager.ts:356-370`). The summary builder and chat system prompt match `ai-service-manager.ts:74-92,120-127`. The locale map matches `locale-map.ts`, with the raw fallback.
- The typed shapes match `schemas.ts`: required and optional fields, the `pending` default, enum casing, and fractional ratings in [1, 5].

No factual question to p23 was needed; every point above cites the source lines it rests on.
