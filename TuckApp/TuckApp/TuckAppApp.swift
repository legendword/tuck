import SwiftUI

@main
struct TuckAppApp: App {
    @State private var tuckService = TuckService()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(tuckService)
        }
    }
}
