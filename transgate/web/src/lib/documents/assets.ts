// Same-origin URLs of the document assets owned by p1W in transgate/export
// (docs/plans/p5-web/implementation/documents.md, P5 D18). One authoritative
// copy: Vite emits each file into dist/_app with a hashed name; there is no
// second copy under public/. `?url` imports only a string, so nothing here puts
// font bytes in the app bundle or fetches anything: p22 fetches the base set
// before the first PDF and only the missing fallback families a document
// requires (the 16.5 MB CJK face is never part of the initial payload).
// File contents and licenses are p1W's; this module owns the URLs.
import type { DocumentAssetUrls } from "$lib/core";
import notoSans from "../../../../export/fonts/NotoSans.ttf?url";
import notoSansItalic from "../../../../export/fonts/NotoSans-Italic.ttf?url";
import notoSansArabic from "../../../../export/fonts/NotoSansArabic.ttf?url";
import notoSansSymbols2 from "../../../../export/fonts/NotoSansSymbols2-Regular.ttf?url";
import notoSansSymbols from "../../../../export/fonts/NotoSansSymbols.ttf?url";
import notoSansHebrew from "../../../../export/fonts/NotoSansHebrew.ttf?url";
import notoSansArmenian from "../../../../export/fonts/NotoSansArmenian.ttf?url";
import notoSansGeorgian from "../../../../export/fonts/NotoSansGeorgian.ttf?url";
import notoSansDevanagari from "../../../../export/fonts/NotoSansDevanagari.ttf?url";
import notoSansKannada from "../../../../export/fonts/NotoSansKannada.ttf?url";
import notoSansTamil from "../../../../export/fonts/NotoSansTamil.ttf?url";
import notoSansThai from "../../../../export/fonts/NotoSansThai.ttf?url";
import notoSansCjk from "../../../../export/fonts/NotoSansCJKjp-Regular.otf?url";
import notoSansLicense from "../../../../export/fonts/NotoSans-OFL.txt?url";
import notoSansArabicLicense from "../../../../export/fonts/NotoSansArabic-OFL.txt?url";
import notoSansSymbols2License from "../../../../export/fonts/NotoSansSymbols2-OFL.txt?url";
import notoSansSymbolsLicense from "../../../../export/fonts/NotoSansSymbols-OFL.txt?url";
import notoSansHebrewLicense from "../../../../export/fonts/NotoSansHebrew-OFL.txt?url";
import notoSansArmenianLicense from "../../../../export/fonts/NotoSansArmenian-OFL.txt?url";
import notoSansGeorgianLicense from "../../../../export/fonts/NotoSansGeorgian-OFL.txt?url";
import notoSansDevanagariLicense from "../../../../export/fonts/NotoSansDevanagari-OFL.txt?url";
import notoSansKannadaLicense from "../../../../export/fonts/NotoSansKannada-OFL.txt?url";
import notoSansTamilLicense from "../../../../export/fonts/NotoSansTamil-OFL.txt?url";
import notoSansThaiLicense from "../../../../export/fonts/NotoSansThai-OFL.txt?url";
import notoSansCjkLicense from "../../../../export/fonts/NotoSansCJK-OFL.txt?url";
import notoSansCjkCopyright from "../../../../export/fonts/NotoSansCJK-COPYRIGHT.txt?url";
import invoiceLogo from "../../../../export/assets/invoice-logo.jpg?url";

/**
 * p22's `TransgateConfig.documentAssets`: same-origin URLs only. Rust fetches
 * `base` on the first PDF and each fallback family only when a captured
 * document needs it (p1W's `FallbackFont::filename` map, documents.md D18).
 */
export const documentAssetUrls: DocumentAssetUrls = {
  base: {
    latin: notoSans,
    latinItalic: notoSansItalic,
    arabic: notoSansArabic,
    symbols: notoSansSymbols2,
    arrows: notoSansSymbols,
    invoiceLogo,
  },
  fallbacks: {
    hebrew: notoSansHebrew,
    armenian: notoSansArmenian,
    georgian: notoSansGeorgian,
    devanagari: notoSansDevanagari,
    kannada: notoSansKannada,
    tamil: notoSansTamil,
    thai: notoSansThai,
    cjk: notoSansCjk,
  },
};

/** Every shipped license notice, for the app's font licenses link. */
export const fontLicenses: readonly { family: string; notices: readonly string[] }[] = [
  { family: "Noto Sans", notices: [notoSansLicense] },
  { family: "Noto Sans Arabic", notices: [notoSansArabicLicense] },
  { family: "Noto Sans Symbols 2", notices: [notoSansSymbols2License] },
  { family: "Noto Sans Symbols", notices: [notoSansSymbolsLicense] },
  { family: "Noto Sans Hebrew", notices: [notoSansHebrewLicense] },
  { family: "Noto Sans Armenian", notices: [notoSansArmenianLicense] },
  { family: "Noto Sans Georgian", notices: [notoSansGeorgianLicense] },
  { family: "Noto Sans Devanagari", notices: [notoSansDevanagariLicense] },
  { family: "Noto Sans Kannada", notices: [notoSansKannadaLicense] },
  { family: "Noto Sans Tamil", notices: [notoSansTamilLicense] },
  { family: "Noto Sans Thai", notices: [notoSansThaiLicense] },
  { family: "Noto Sans CJK", notices: [notoSansCjkLicense, notoSansCjkCopyright] },
];
