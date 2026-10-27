//! Consumer-boundary regressions for analysis and invoice exports (P5-DOCS-FINAL-A,
//! D3, D22 and session fences). The real `DocumentWriter`, billing/W fixtures and
//! licensed fonts are used; only the asset load is held, as a slow same-origin fetch.
//! The driver uses the analysis AI epoch or invoice session epoch, checked with
//! real core `AppState::is_current`, plus disposal. Every session or consent
//! change below is a real core transition, not a counter.
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use billing_core::presentation::invoices::{DocumentPurpose, InvoicesEffect, InvoicesEvent};
use billing_core::presentation::purchase::PurchaseFailure;
use serde::Deserialize;
use tokio::sync::Notify;
use transgate_client::export::{Delivery, DocumentAssetSource, DocumentFonts, export_analysis, export_invoice};
use transgate_client::future::BoxFuture;
use transgate_core::ai::ConsentView;
use transgate_core::ai::analysis::AnalysisProgress;
use transgate_core::ai::ports::GenerationStatus;
use transgate_core::documents::{AnalysisDocument, AnalysisMetadata, DocumentFormat, InvoiceDocument};
use transgate_core::presentation::ai::consent;
use transgate_core::presentation::app::{AppConfig, AppEffect, AppEvent, AppIntent, AppState, AppVm, AuthEvent, Epoch, InvoicesIntent, SessionFacts};
use transgate_core::presentation::layout::Viewport;
use transgate_core::presentation::route::Route;
use transgate_core::{JobId, Micros};
use transgate_export::documents::{DocumentAssets, FallbackFont, FontBytes};
use uuid::Uuid;

const NOW: Micros = Micros(1_700_000_000_000_000);

fn assets() -> DocumentAssets {
    DocumentAssets {
        fonts: FontBytes {
            latin: include_bytes!("../../export/fonts/NotoSans.ttf").to_vec(),
            latin_italic: include_bytes!("../../export/fonts/NotoSans-Italic.ttf").to_vec(),
            arabic: include_bytes!("../../export/fonts/NotoSansArabic.ttf").to_vec(),
            symbols: include_bytes!("../../export/fonts/NotoSansSymbols2-Regular.ttf").to_vec(),
            arrows: include_bytes!("../../export/fonts/NotoSansSymbols.ttf").to_vec(),
        },
        invoice_logo: include_bytes!("../../export/assets/invoice-logo.jpg").to_vec(),
    }
}

/// The bundled assets, with an optional hold on the base load.
#[derive(Default)]
struct Bundle {
    hold: AtomicBool,
    base_loads: AtomicUsize,
    fallback_loads: AtomicUsize,
    entered: Notify,
    release: Notify,
}

impl DocumentAssetSource for Bundle {
    fn base(&self) -> BoxFuture<'_, Result<DocumentAssets, String>> {
        Box::pin(async move {
            self.base_loads.fetch_add(1, Ordering::SeqCst);
            if self.hold.load(Ordering::SeqCst) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            Ok(assets())
        })
    }
    fn fallback(&self, family: FallbackFont) -> BoxFuture<'_, Result<Vec<u8>, String>> {
        Box::pin(async move {
            self.fallback_loads.fetch_add(1, Ordering::SeqCst);
            Err(format!("{} is not needed by a Latin capture", family.filename()))
        })
    }
}

fn owner(user: u128, expires_at: Micros) -> SessionFacts {
    SessionFacts { user_id: Uuid::from_u128(user), email: format!("user{user}@example.test"), expires_at }
}

fn consent_view(granted: bool, cleanup_pending: bool) -> ConsentView {
    ConsentView {
        has_consented: granted,
        consent_version: "1.0".into(),
        consent_date: granted.then_some(NOW),
        revoked_date: (!granted).then_some(NOW),
        privacy_policy_url: "/legal/privacy-policy-en".into(),
        provider_name: "Provider".into(),
        can_revoke: granted,
        cleanup_pending,
    }
}

/// The driver's view of "who may still receive this capture": real core state.
struct Presenter {
    state: RefCell<AppState>,
    disposed: Cell<bool>,
}

impl Presenter {
    /// User 1 signed in with AI consent granted, as when the export is clicked.
    fn signed_in() -> Presenter {
        let p = Presenter {
            state: RefCell::new(AppState::new(AppConfig { site_url: "https://app.test".into(), captcha: false })),
            disposed: Cell::new(false),
        };
        p.apply(AppEvent::User(AppIntent::Started { url: "/home".into(), viewport: Viewport::default(), utc_offset_minutes: 0 }));
        p.auth(AuthEvent::Restored { facts: Some(owner(1, Micros(NOW.0 + 3_600_000_000))) });
        p.consent(consent::Event::Loaded { result: Ok(consent_view(true, false)) });
        p
    }
    fn apply(&self, event: AppEvent) {
        self.state.borrow_mut().update(event, NOW);
    }
    fn auth(&self, event: AuthEvent) {
        let epoch = self.state.borrow().epoch();
        self.apply(AppEvent::Auth { epoch, event });
    }
    fn consent(&self, event: consent::Event) {
        let epoch = self.state.borrow().epoch();
        self.apply(AppEvent::Consent { epoch, event });
    }
    /// The fence an analysis export effect carries.
    fn ticket(&self) -> Epoch {
        self.state.borrow().ai_epoch()
    }
    fn current(&self, ticket: Epoch) -> bool {
        !self.disposed.get() && self.state.borrow().is_current(ticket)
    }
}

#[derive(Deserialize)]
struct Case {
    name: String,
    progress: AnalysisProgress,
}

/// The clicked partial, captured once (title and instant are the click's).
fn clicked() -> AnalysisDocument {
    let cases: Vec<Case> = serde_json::from_str(include_str!("../../core/src/documents/fixtures/analysis.json")).unwrap();
    let partial = cases.into_iter().find(|c| c.name == "partial").expect("W's partial fixture");
    let metadata = AnalysisMetadata { title: "Clicked title".into(), job_id: Some(JobId(7)), exported_at: Micros(1_700_000_000_000_000), utc_offset_minutes: 180 };
    AnalysisDocument::capture(&partial.progress, GenerationStatus::Interrupted, metadata).unwrap()
}

/// Runs one PDF export whose asset load is held until `during` has changed the presenter.
async fn held_pdf(bundle: &Arc<Bundle>, fonts: &DocumentFonts, presenter: &Presenter, during: impl FnOnce(&Presenter)) -> (Delivery, usize) {
    bundle.hold.store(true, Ordering::SeqCst);
    let document = clicked();
    let ticket = presenter.ticket();
    let saves = AtomicUsize::new(0);
    let (delivery, ()) = tokio::join!(
        export_analysis(fonts, &document, DocumentFormat::Pdf, || presenter.current(ticket), |_| {
            saves.fetch_add(1, Ordering::SeqCst);
            true
        }),
        async {
            bundle.entered.notified().await;
            during(presenter);
            bundle.release.notify_one();
        }
    );
    bundle.hold.store(false, Ordering::SeqCst);
    (delivery, saves.load(Ordering::SeqCst))
}

fn setup() -> (Arc<Bundle>, DocumentFonts, Presenter) {
    let bundle = Arc::new(Bundle::default());
    let fonts = DocumentFonts::new(bundle.clone());
    (bundle, fonts, Presenter::signed_in())
}

#[tokio::test]
async fn sign_out_then_same_owner_relogin_during_a_held_font_load_saves_nothing() {
    let (bundle, fonts, presenter) = setup();
    let (delivery, saves) = held_pdf(&bundle, &fonts, &presenter, |p| {
        p.apply(AppEvent::User(AppIntent::SignOutRequested));
        p.auth(AuthEvent::SignedOut);
        // The same owner signs in again and regrants: a new session, not the old one.
        p.auth(AuthEvent::SignedIn { facts: owner(1, Micros(NOW.0 + 7_200_000_000)) });
        p.consent(consent::Event::Loaded { result: Ok(consent_view(true, false)) });
    })
    .await;
    assert_eq!((delivery, saves), (Delivery::Stale, 0));
}

#[tokio::test]
async fn an_account_switch_during_a_held_font_load_never_delivers_the_previous_capture() {
    let (bundle, fonts, presenter) = setup();
    let (delivery, saves) = held_pdf(&bundle, &fonts, &presenter, |p| {
        // Another tab signed in as someone else, who has consented too.
        p.auth(AuthEvent::SessionAdopted { facts: owner(2, Micros(NOW.0 + 3_600_000_000)) });
        p.consent(consent::Event::Loaded { result: Ok(consent_view(true, false)) });
    })
    .await;
    assert_eq!((delivery, saves), (Delivery::Stale, 0));
}

#[tokio::test]
async fn a_consent_refusal_then_regrant_leaves_an_older_capture_stale() {
    let (bundle, fonts, presenter) = setup();
    let (delivery, saves) = held_pdf(&bundle, &fonts, &presenter, |p| {
        // POST {hasConsented: false}: the nondestructive refusal.
        p.consent(consent::Event::DeclineRequested);
        p.consent(consent::Event::DecisionSaved { result: Ok(consent_view(false, false)) });
        p.consent(consent::Event::EnableRequested);
        p.consent(consent::Event::DecisionSaved { result: Ok(consent_view(true, false)) });
    })
    .await;
    assert_eq!((delivery, saves), (Delivery::Stale, 0));
}

#[tokio::test]
async fn turning_ai_off_with_delete_then_regrant_leaves_an_older_capture_stale() {
    let (bundle, fonts, presenter) = setup();
    let (delivery, saves) = held_pdf(&bundle, &fonts, &presenter, |p| {
        // D22: DELETE revokes and purges saved AI data account-side; the purge may still run.
        p.consent(consent::Event::DisableConfirmed);
        p.consent(consent::Event::DecisionSaved { result: Ok(consent_view(false, true)) });
        p.consent(consent::Event::EnableRequested);
        p.consent(consent::Event::DecisionSaved { result: Ok(consent_view(true, false)) });
    })
    .await;
    assert_eq!((delivery, saves), (Delivery::Stale, 0));
}

#[tokio::test]
async fn a_token_rotation_for_the_same_owner_still_delivers_the_clicked_capture() {
    let (bundle, fonts, presenter) = setup();
    let (delivery, saves) = held_pdf(&bundle, &fonts, &presenter, |p| {
        p.auth(AuthEvent::SessionAdopted { facts: owner(1, Micros(NOW.0 + 7_200_000_000)) });
    })
    .await;
    assert_eq!((delivery, saves), (Delivery::Delivered(Ok(())), 1));
}

#[tokio::test]
async fn disposal_during_a_held_load_saves_nothing_and_the_public_fonts_stay_cached() {
    let (bundle, fonts, presenter) = setup();
    let (delivery, saves) = held_pdf(&bundle, &fonts, &presenter, |p| p.disposed.set(true)).await;
    assert_eq!((delivery, saves), (Delivery::Stale, 0));

    // A new presenter reuses the parsed writer: fonts are not private output.
    let next = Presenter::signed_in();
    let document = clicked();
    let delivered = export_analysis(&fonts, &document, DocumentFormat::Pdf, || next.current(next.ticket()), |_| true).await;
    assert_eq!(delivered, Delivery::Delivered(Ok(())));
    assert_eq!(bundle.base_loads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn an_unchanged_session_delivers_the_clicked_snapshot_exactly_once() {
    let (bundle, fonts, presenter) = setup();
    bundle.hold.store(true, Ordering::SeqCst);
    let document = clicked();
    let expected_name = document.filename(DocumentFormat::Pdf);
    let ticket = presenter.ticket();
    let mut saved = Vec::new();
    let (delivery, ()) = tokio::join!(
        export_analysis(&fonts, &document, DocumentFormat::Pdf, || presenter.current(ticket), |bytes| {
            saved.push(bytes);
            true
        }),
        async {
            bundle.entered.notified().await;
            bundle.release.notify_one();
        }
    );
    assert_eq!(delivery, Delivery::Delivered(Ok(())));
    let [bytes] = saved.as_slice() else { panic!("exactly one save") };
    assert_eq!((bytes.filename.as_str(), bytes.content_type.as_str()), (expected_name.as_str(), "application/pdf"));
    assert!(bytes.bytes.starts_with(b"%PDF"));
    assert_eq!(bundle.fallback_loads.load(Ordering::SeqCst), 0, "a Latin capture needs no extra script family");
}

#[tokio::test]
async fn a_failed_save_is_reported_as_a_save_failure() {
    let (bundle, fonts, presenter) = setup();
    let document = clicked();
    let failed = export_analysis(&fonts, &document, DocumentFormat::Pdf, || presenter.current(presenter.ticket()), |_| false).await;
    assert_eq!(failed, Delivery::Delivered(Err(transgate_core::presentation::ai::analysis::ExportFailure::Save)));
    assert_eq!(bundle.base_loads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn font_free_formats_render_without_fetching_any_pdf_asset() {
    let (bundle, fonts, presenter) = setup();
    let document = clicked();
    for format in [DocumentFormat::Docx, DocumentFormat::Txt, DocumentFormat::Md] {
        let delivery = export_analysis(&fonts, &document, format, || presenter.current(presenter.ticket()), |_| true).await;
        assert_eq!(delivery, Delivery::Delivered(Ok(())), "{format:?}");
    }
    assert_eq!(bundle.base_loads.load(Ordering::SeqCst) + bundle.fallback_loads.load(Ordering::SeqCst), 0);
}

fn clicked_invoice() -> InvoiceDocument {
    let context = billing_core::presentation::fixtures::invoice_context_sample();
    InvoiceDocument::capture(&context.invoices[0], &context.bill_to, "Clicked Company\nClicked Address").unwrap()
}

#[tokio::test]
async fn an_invoice_held_across_same_owner_relogin_is_never_delivered() {
    let (bundle, fonts, presenter) = setup();
    bundle.hold.store(true, Ordering::SeqCst);
    let document = clicked_invoice();
    let epoch = presenter.state.borrow().epoch();
    let mut deliveries = 0;
    let (delivery, ()) = tokio::join!(
        export_invoice(&fonts, &document, || presenter.current(epoch), |_| {
            deliveries += 1;
            true
        }),
        async {
            bundle.entered.notified().await;
            presenter.apply(AppEvent::User(AppIntent::SignOutRequested));
            presenter.auth(AuthEvent::SignedOut);
            presenter.auth(AuthEvent::SignedIn { facts: owner(1, Micros(NOW.0 + 7_200_000_000)) });
            bundle.release.notify_one();
        }
    );
    assert_eq!(delivery, Delivery::Stale);
    assert_eq!(deliveries, 0);
}

#[tokio::test]
async fn an_invoice_uses_the_session_fence_not_ai_consent() {
    let (bundle, fonts, presenter) = setup();
    bundle.hold.store(true, Ordering::SeqCst);
    let document = clicked_invoice();
    let epoch = presenter.state.borrow().epoch();
    let mut saved = None;
    let (delivery, ()) = tokio::join!(
        export_invoice(&fonts, &document, || presenter.current(epoch), |bytes| {
            saved = Some(bytes);
            true
        }),
        async {
            bundle.entered.notified().await;
            presenter.consent(consent::Event::DeclineRequested);
            presenter.consent(consent::Event::DecisionSaved { result: Ok(consent_view(false, false)) });
            bundle.release.notify_one();
        }
    );
    assert_eq!(delivery, Delivery::Delivered(Ok(())));
    let bytes = saved.expect("the invoice remains available after refusing AI consent");
    assert_eq!(bytes.filename, document.filename());
    assert_eq!(bytes.content_type, "application/pdf");
    assert!(bytes.bytes.starts_with(b"%PDF"));
}

/// Uses the ticket on the originating core effect, never a later state reread.
fn invoice_work(presenter: &Presenter, event: AppEvent) -> (Epoch, u64, InvoicesEffect) {
    presenter.state.borrow_mut().update(event, NOW).into_iter().find_map(|effect| match effect {
        AppEffect::Invoices { epoch, ticket, effect } => Some((epoch, ticket, *effect)),
        _ => None,
    }).expect("invoice work emitted by core")
}

fn invoice_capture(effect: InvoicesEffect) -> InvoiceDocument {
    let InvoicesEffect::Document { facts, bill_to, temporary_bill_to, purpose: DocumentPurpose::Preview } = effect else {
        panic!("preview capture expected");
    };
    InvoiceDocument::capture(&facts, &bill_to, &temporary_bill_to).unwrap()
}

#[tokio::test]
async fn closing_and_reopening_the_same_invoice_fences_held_bytes_and_old_failure() {
    let (bundle, fonts, presenter) = setup();
    let epoch = presenter.state.borrow().epoch();
    presenter.apply(AppEvent::SessionLoaded { epoch, result: Ok(transgate_core::presentation::fixtures::session_profile()) });
    let (epoch, ticket, effect) = invoice_work(&presenter, AppEvent::User(AppIntent::Navigate { route: Route::Billing }));
    assert!(matches!(effect, InvoicesEffect::LoadOwn));
    let context = billing_core::presentation::fixtures::invoice_context_sample();
    let payment_id = context.invoices[0].payment_id;
    presenter.apply(AppEvent::Invoices { epoch, ticket, event: InvoicesEvent::Loaded(Ok(context)) });
    let preview = || AppEvent::User(AppIntent::Invoices(InvoicesIntent::Preview { payment_id }));
    let (epoch, ticket, effect) = invoice_work(&presenter, preview());
    let captured = invoice_capture(effect);
    bundle.hold.store(true, Ordering::SeqCst);
    let mut delivered = 0;
    let mut next = None;
    let (delivery, ()) = tokio::join!(
        export_invoice(
            &fonts,
            &captured,
            || presenter.current(epoch) && presenter.state.borrow().is_current_invoice_preview(ticket, payment_id),
            |_| { delivered += 1; true },
        ),
        async {
            bundle.entered.notified().await;
            presenter.apply(AppEvent::User(AppIntent::Invoices(InvoicesIntent::ClosePreview)));
            let (next_epoch, next_ticket, effect) = invoice_work(&presenter, preview());
            presenter.apply(AppEvent::Invoices {
                epoch,
                ticket,
                event: InvoicesEvent::DocumentFailed { payment_id, failure: PurchaseFailure::Unavailable },
            });
            assert!(presenter.state.borrow().is_current_invoice_preview(next_ticket, payment_id));
            let AppVm::SignedIn(vm) = presenter.state.borrow().view(NOW) else { panic!("signed in") };
            assert!(vm.invoices.unwrap().document_failure.is_none(), "an old failure must not clear or fail the newer preview");
            next = Some((next_epoch, next_ticket, invoice_capture(effect)));
            bundle.release.notify_one();
        },
    );
    assert_eq!(delivery, Delivery::Stale);
    assert_eq!(delivered, 0, "the closed request must never install bytes into the reopened modal");

    let (epoch, ticket, captured) = next.unwrap();
    let delivery = export_invoice(
        &fonts,
        &captured,
        || presenter.current(epoch) && presenter.state.borrow().is_current_invoice_preview(ticket, payment_id),
        |_| { delivered += 1; true },
    ).await;
    assert_eq!(delivery, Delivery::Delivered(Ok(())));
    assert_eq!(delivered, 1, "the new request still delivers using the cached public fonts");
}
