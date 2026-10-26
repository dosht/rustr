import SwiftUI

/// Dynamic generated colors follow the system or an explicit preferredColorScheme.
struct TaskrTheme: ViewModifier {
    func body(content: Content) -> some View {
        content
            .tint(DesignTokens.current.accent)
            .foregroundStyle(DesignTokens.current.fg)
            .background(DesignTokens.current.bg.ignoresSafeArea())
    }
}
