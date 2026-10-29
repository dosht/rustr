import SwiftUI

struct LocaleMenu: View {
    let locales: [LocaleOption]
    let copy: Copy
    let send: (AppIntent) -> Void

    var body: some View {
        Menu {
            ForEach(locales, id: \.code) { option in
                Button {
                    send(.localeChosen(code: option.code))
                } label: {
                    if option.current {
                        SwiftUI.Label("\(option.flag) \(copy.text(option.name))", systemImage: "checkmark")
                    } else {
                        Text("\(option.flag) \(copy.text(option.name))")
                    }
                }
            }
        } label: {
            HStack {
                Text(copy.text(Labels.changeLanguage))
                if let current = locales.first(where: \.current) {
                    Text("\(current.flag) \(copy.text(current.name))")
                }
            }
            .frame(minHeight: 44)
        }
        .accessibilityIdentifier("locale-picker")
    }
}
