import SwiftUI

/// The signed-in app. Which screen shows comes from Rust; a split view with a
/// sidebar is how SwiftUI shows it (a stack on iPhone, columns on iPad).
struct AppRootView: View {
    let view: AppView
    let devSignIn: Bool
    let onIntent: (AppIntent) -> Void

    var body: some View {
        Group {
            switch view {
            case .starting:
                ProgressView()
            case .signedOut(let signIn):
                SignInScreen(view: signIn, devSignIn: devSignIn, onIntent: onIntent)
            case .signedIn(let signedIn):
                AppFrame(view: signedIn, onIntent: onIntent)
            }
        }
        .modifier(TaskrTheme())
    }
}

struct SignInScreen: View {
    let view: SignInView
    let devSignIn: Bool
    let onIntent: (AppIntent) -> Void

    @State private var email = ""
    @State private var password = ""

    var body: some View {
        Form {
            Section {
                TextField("Email", text: $email)
                    .textContentType(.emailAddress)
                    .keyboardType(.emailAddress)
                    .textInputAutocapitalization(.never)
                if !devSignIn {
                    SecureField("Password", text: $password)
                        .textContentType(.password)
                }
            } header: {
                Text("taskr").font(.largeTitle.bold()).foregroundStyle(DesignTokens.current.accent)
            } footer: {
                if devSignIn { Text("Dev sign-in: any email, no password.") }
            }
            .listRowBackground(DesignTokens.current.surface)
            if let message = view.error ?? view.notice {
                Text(message).foregroundStyle(view.error != nil ? DesignTokens.current.error : DesignTokens.current.fg)
                    .listRowBackground(view.error != nil ? DesignTokens.current.errorBg : DesignTokens.current.noticeBg)
            }
            Section {
                Button("Sign in") { onIntent(.signInSubmitted(email: email, password: password)) }
                if !devSignIn {
                    Button("Create account") { onIntent(.signUpSubmitted(email: email, password: password)) }
                }
            }
            .disabled(view.busy)
            .listRowBackground(DesignTokens.current.surface)
        }
        .scrollContentBackground(.hidden)
        .background(DesignTokens.current.bg)
    }
}

struct AppFrame: View {
    let view: SignedInView
    let onIntent: (AppIntent) -> Void

    var body: some View {
        NavigationSplitView {
            List {
                Section("Workspaces") {
                    ForEach(view.workspaces, id: \.id) { w in
                        Button {
                            onIntent(.workspaceSelected(id: w.id))
                        } label: {
                            HStack {
                                Text(w.name).fontWeight(w.selected ? .semibold : .regular)
                                Spacer()
                                Text(w.roleLabel).font(.caption).foregroundStyle(DesignTokens.current.muted)
                            }
                        }
                    }
                }
                .listRowBackground(DesignTokens.current.surface)
                Section("Sections") {
                    ForEach(view.screens, id: \.label) { s in
                        Button {
                            onIntent(.screenSelected(screen: s.screen))
                        } label: {
                            Text(s.label).fontWeight(s.selected ? .semibold : .regular)
                        }
                    }
                }
                .listRowBackground(DesignTokens.current.surface)
                Section(view.email) {
                    Button("Sign out", role: .destructive) { onIntent(.signOutRequested) }
                }
                .listRowBackground(DesignTokens.current.surface)
            }
            .scrollContentBackground(.hidden)
            .background(DesignTokens.current.bg)
            .navigationTitle("taskr")
        } detail: {
            VStack(spacing: 0) {
                if let error = view.error {
                    HStack {
                        Text(error).foregroundStyle(DesignTokens.current.error)
                        Spacer()
                        Button("Dismiss") { onIntent(.messageDismissed) }
                    }
                    .padding()
                    .background(DesignTokens.current.errorBg)
                }
                switch view.screen {
                case .loading:
                    ProgressView().frame(maxHeight: .infinity)
                case .tasks(let tasks):
                    TaskListContent(view: tasks) { intent in
                        if let ws = view.workspace { onIntent(.tasks(workspace: ws, intent: intent)) }
                    }
                case .members(let members):
                    MembersContent(view: members) { intent in
                        if let ws = view.workspace { onIntent(.members(workspace: ws, intent: intent)) }
                    }
                }
            }
            .navigationTitle(view.title)
        }
    }
}

/// Pure view: renders a MembersView. What an owner can do comes from `canManage`.
struct MembersContent: View {
    let view: MembersView
    let onIntent: (MembersIntent) -> Void

    @State private var email = ""
    @State private var role: MemberRole = .editor

    var body: some View {
        List {
            if view.canManage {
                Section("Add a member") {
                    TextField("Email", text: $email)
                        .keyboardType(.emailAddress)
                        .textInputAutocapitalization(.never)
                    RolePicker(selection: $role, options: view.roleOptions)
                    Button("Add") {
                        onIntent(.addSubmitted(email: email, role: role))
                        email = ""
                    }
                }
                .listRowBackground(DesignTokens.current.surface)
            }
            if let message = view.error ?? view.notice {
                HStack {
                    Text(message).foregroundStyle(view.error != nil ? DesignTokens.current.error : DesignTokens.current.fg)
                    Spacer()
                    Button("Dismiss") { onIntent(.messageDismissed) }
                }
                .listRowBackground(view.error != nil ? DesignTokens.current.errorBg : DesignTokens.current.noticeBg)
            }
            Section("Members") {
                ForEach(view.rows, id: \.email) { row in
                    HStack {
                        Text(row.isYou ? "\(row.email) (you)" : row.email)
                        Spacer()
                        if view.canManage {
                            RolePicker(
                                selection: Binding(
                                    get: { row.role },
                                    set: { onIntent(.roleChanged(email: row.email, role: $0)) }
                                ),
                                options: view.roleOptions
                            )
                            .labelsHidden()
                        } else {
                            Text(row.roleLabel).foregroundStyle(DesignTokens.current.muted)
                        }
                    }
                    .swipeActions {
                        if view.canManage {
                            Button("Remove", role: .destructive) {
                                onIntent(.removeRequested(email: row.email))
                            }
                        }
                    }
                }
            }
            .listRowBackground(DesignTokens.current.surface)
        }
        .scrollContentBackground(.hidden)
        .background(DesignTokens.current.bg)
    }
}

private struct RolePicker: View {
    @Binding var selection: MemberRole
    let options: [RoleOptionView]

    var body: some View {
        Picker("Role", selection: $selection) {
            ForEach(options, id: \.label) { option in
                Text(option.label).tag(option.role)
            }
        }
    }
}

// The same named screens the web's Storybook and the Compose previews render.
private func preview(_ name: String) -> AppView {
    appPreviewScreens().first { $0.name == name }?.view ?? .starting
}

#Preview("Sign in") {
    AppRootView(view: preview("sign-in"), devSignIn: false) { _ in }
}

#Preview("Owner") {
    AppRootView(view: preview("owner-tasks"), devSignIn: false) { _ in }
}

#Preview("Viewer") {
    AppRootView(view: preview("viewer-tasks"), devSignIn: false) { _ in }
}

#Preview("Members") {
    AppRootView(view: preview("owner-members"), devSignIn: false) { _ in }
}
