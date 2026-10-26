# P2 exact-money investigation

Observed locally with Node v26.2.0 on 2026-10-24. No provider calls, production data or application edits.

## Source and method

Compared the current expressions in `frontend/src/pages/checkout/index.tsx:288-313`, `src/domain/packages.ts:73-75`, `src/domain/coupon.ts:3-4`, `src/utils/formats.ts:28` and `src/domain/currency.ts:1-3` with integer-rational evaluation of the same operation order. Inputs: every integer hour 5 through 9999, locale discount off/on, matching coupon absent/10%/25%/33%, special offer off/on, requested USD/TRY, rate 44. This is 319840 parameter combinations, not distinct customers or transactions. The special offer ignores coupon/locale/requested currency, as source does.

Legacy: JavaScript `Number`, `Math.floor` at locale discount, `Math.round` at price/coupon rounding, then TRY multiplication. Exact: base cents as BigInt, division by two with floor for the locale discount, half-up integer percentage rounding for coupon, then multiplication by 44. Final comparison quantizes legacy to minor units with `Math.round(amount * 100)`.

## Observed result

3824 combinations differ. Maximum absolute difference is 44 minor units (0.44 TRY). These are not all explainable as insignificant decimal serialization artifacts.

| Hours | Locale discount | Coupon | Currency | Legacy amount | Exact amount |
|---|---|---|---|---|---|
| 33 | yes | 25% | USD | 18.43 | 18.44 |
| 34 | no | 25% | USD | 37.99 | 38.00 |
| 50 | no | 33% | USD | 49.91 | 49.92 |
| 54 | yes | absent | USD | 40.22 | 40.23 |
| 54 | yes | absent | TRY | 1769.68 | 1770.12 |

The 54-hour case settles Opus's open floor counterexample: `54 * 1.49`, then 50% discount and cent-floor, loses a cent through binary representation. The mathematical amount before floor is exactly 40.23. The coupon cases similarly cross a half-cent boundary through floating-point error.

## Implementation implication

Do not assert a full legacy-equals-decimal golden oracle. Keep a source-derived legacy comparison fixture, classify these differences, and test exact pricing separately against independent rational expectations. Lead must record the money outcome decision before implementation. Preserve historical stored amounts; distinguish quoted amount from a verified observed provider amount. A provider signature that covers only identifiers does not establish an observed amount.
