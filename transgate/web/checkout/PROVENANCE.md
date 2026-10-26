# Checkout document asset provenance

Not Vite-built and not served itself; the host serves `public/_app/*` as unhashed, revalidated (`no-cache`) files.

## `public/_app/iyzico-secure-payment.svg`

- What: Iyzico's "Güvenli Ödeme" (secure payment) trust badge, the footer strip of both checkout outcomes in `checkout/document.html`.
- Source URL: `https://cpp.iyzipay.com/img/guvenli_odeme_logo_seridi.svg` (public, fetched once, unmodified).
- Retrieved: 2026-10-26.
- SHA-256: `39233f34c10473013df18aa07c107767d82b55f1a9fe6a3f21ff5edfa40c9405` (656,139 bytes).
- Use in the legacy app: `Transgate/frontend/src/pages/checkout/index.tsx:123` and `src/pages/billing/index.tsx:707`, as a remote `<img>`.
- Why local: P5 D20 forbids CDN/third-party fetches for app-owned assets; candidate6 C2/C3 saw the page requesting the remote URL. The checkout CSP is unchanged (D19 keeps `img-src https:` for Iyzico's own injected form).
- Inspection: no `<script>`, event handlers, `foreignObject` or external references; its six embedded raster images are `data:image/png;base64` URIs. Brand artwork belongs to Iyzico; no license text was published with it. It is used only as the payment-provider trust mark.
- Updating: replace in place and update the hash above.
