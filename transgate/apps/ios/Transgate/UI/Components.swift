import SwiftUI

/// Shared, state-free building blocks. Colors come from the generated tokens and
/// core's semantic `Tone`, never from literals.
let tokens = DesignTokens.current

struct PrimaryButton: View {
    let title: String
    var busy = false
    var enabled = true
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                if busy { ProgressView().tint(tokens.onAccent) }
                Text(title).fontWeight(.semibold)
            }
            .frame(maxWidth: .infinity, minHeight: 44)
        }
        .buttonStyle(.borderedProminent)
        .tint(tokens.accent)
        .foregroundStyle(tokens.onAccent)
        .disabled(!enabled || busy)
        .accessibilityAddTraits(busy ? .updatesFrequently : [])
    }
}

struct SecondaryButton: View {
    let title: String
    var destructive = false
    var enabled = true
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title).frame(maxWidth: .infinity, minHeight: 44)
        }
        .buttonStyle(.bordered)
        .tint(destructive ? tokens.error : tokens.accent)
        .disabled(!enabled)
    }
}

/// A one-line message from core, colored by tone or by error/notice meaning.
struct Banner: View {
    enum Kind { case info, success, error }
    let text: String
    var kind: Kind = .info
    var dismissLabel = ""
    var dismiss: (() -> Void)?

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Text(text).foregroundStyle(tokens.fg).frame(maxWidth: .infinity, alignment: .leading)
            if let dismiss {
                Button(action: dismiss) { Image(systemName: "xmark") }.foregroundStyle(tokens.muted)
                    .accessibilityLabel(dismissLabel)
            }
        }
        .padding(12)
        .background(kind == .error ? tokens.errorBg : tokens.noticeBg, in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(kind == .error ? tokens.error : kind == .success ? tokens.success : tokens.border))
        .accessibilityElement(children: .combine)
    }
}

struct ToneBadge: View {
    let text: String
    let tone: Tone
    var spinner = false
    /// Core's glyph for the badge (`BadgeIcon`); the shell only picks a symbol for it.
    var icon: BadgeIcon? = nil

    var body: some View {
        HStack(spacing: 4) {
            if spinner { ProgressView().scaleEffect(0.6) }
            else if let icon {
                switch icon {
                case .dot: Circle().frame(width: 6, height: 6)
                case .upload: Image(systemName: "arrow.up").font(.caption2)
                case .check: Image(systemName: "checkmark").font(.caption2)
                case .cross: Image(systemName: "xmark").font(.caption2)
                }
            }
            Text(text).font(.caption.weight(.semibold))
        }
        .padding(.horizontal, 8).padding(.vertical, 3)
        .foregroundStyle(tokens.tone(tone))
        .overlay(Capsule().stroke(tokens.tone(tone)))
    }
}

struct LabeledField: View {
    let label: String
    @Binding var text: String
    var secure = false
    var keyboard: UIKeyboardType = .default
    var contentType: UITextContentType?
    var multiline = false
    var enabled = true

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label).font(.subheadline).foregroundStyle(tokens.muted)
            Group {
                if secure { SecureField(label, text: $text) }
                else if multiline { TextField(label, text: $text, axis: .vertical).lineLimit(3...8) }
                else { TextField(label, text: $text) }
            }
            .textContentType(contentType)
            .keyboardType(keyboard)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .padding(10)
            .background(tokens.surface, in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(tokens.border))
            .disabled(!enabled)
        }
    }
}

extension View {
    /// The screen background every top-level screen sits on.
    func screenBackground() -> some View {
        background(tokens.bg.ignoresSafeArea()).foregroundStyle(tokens.fg)
    }
}

/// A binding that reads core's value and reports a change as an intent. The view keeps no
/// copy of the text, so core's value is the one on screen.
func coreText(_ value: String, _ send: @escaping (String) -> Void) -> Binding<String> {
    Binding(get: { value }, set: send)
}

func coreFlag(_ value: Bool, _ send: @escaping (Bool) -> Void) -> Binding<Bool> {
    Binding(get: { value }, set: send)
}
