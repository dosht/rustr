import SwiftUI

/// Sign in, sign up, magic link and forgotten password (WEB-052). Core's `AuthVm`
/// decides the mode, the busy and submit states, errors and notices; the labels are
/// the source's `auth` keys for the shown mode.
struct AuthScreen: View {
    let vm: AuthVm
    let copy: Copy
    let send: (AppIntent) -> Void

    private var mode: AuthMode { vm.mode }
    private var social: Bool { vm.mode == .signIn || vm.mode == .signUp }
    private var withPassword: Bool { vm.mode == .signIn || vm.mode == .signUp }

    /// The source's links under the form, per view; each is labeled with its target's `link_text`.
    private var links: [AuthMode] {
        switch vm.mode {
        case .signIn: return [.magicLink, .forgottenPassword, .signUp]
        case .signUp: return [.magicLink, .signIn]
        default: return [.signIn]
        }
    }

    var body: some View {
        ScrollView {
            VStack(spacing: 20) {
                Image("logo").resizable().scaledToFit().frame(height: 56).padding(.vertical, 24).accessibilityLabel("Transgate")
                if social {
                    SecondaryButton(title: copy.text(Labels.socialLabel(mode)), enabled: !vm.busy) { send(.auth(.googleChosen)) }
                    Divider()
                }
                LabeledField(label: copy.text(Labels.emailLabel(mode)), text: coreText(vm.email) { send(.auth(.emailChanged(text: $0))) },
                             keyboard: .emailAddress, contentType: .emailAddress, enabled: !vm.busy)
                if withPassword {
                    LabeledField(label: copy.text(Labels.auth(mode, "password_label")), text: coreText(vm.password) { send(.auth(.passwordChanged(text: $0))) },
                                 secure: true, contentType: mode == .signUp ? .newPassword : .password, enabled: !vm.busy)
                }
                PrimaryButton(title: copy.text(vm.busy ? Labels.auth(mode, "loading_button_label") : Labels.auth(mode, "button_label")),
                              busy: vm.busy, enabled: vm.canSubmit) { send(.auth(.submitted)) }
                if let error = copy.text(vm.error) { Banner(text: error, kind: .error) }
                if let notice = copy.text(vm.notice) { Banner(text: notice, kind: .success) }
                VStack(spacing: 4) {
                    ForEach(links, id: \.self) { target in
                        Button(copy.text(Labels.auth(target, "link_text"))) { send(.auth(.modeChosen(mode: target))) }
                            .foregroundStyle(tokens.muted)
                            .frame(minHeight: 44)
                    }
                }
            }
            .padding(20)
        }
        .screenBackground()
    }
}

/// The recovery link's update-password screen (WEB-052).
struct NewPasswordScreen: View {
    let vm: NewPasswordVm
    let copy: Copy
    let send: (AppIntent) -> Void

    var body: some View {
        ScrollView {
            VStack(spacing: 20) {
                Image("logo").resizable().scaledToFit().frame(height: 56).padding(.vertical, 24).accessibilityLabel("Transgate")
                LabeledField(label: copy.text(Labels.newPassword), text: coreText(vm.password) { send(.newPassword(.passwordChanged(text: $0))) },
                             secure: true, contentType: .newPassword, enabled: !vm.busy)
                if let error = copy.text(vm.error) { Banner(text: error, kind: .error) }
                if let success = copy.text(vm.success) { Banner(text: success, kind: .success) }
                PrimaryButton(title: copy.text(Labels.changePassword), busy: vm.busy) { send(.newPassword(.submitted)) }
            }
            .padding(20)
        }
        .screenBackground()
    }
}
