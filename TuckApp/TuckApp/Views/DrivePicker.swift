import SwiftUI

struct DrivePicker: View {
    @Environment(TuckService.self) private var tuckService

    var body: some View {
        @Bindable var service = tuckService
        Picker("Drive", selection: Binding(
            get: { tuckService.selectedDrive?.name ?? "" },
            set: { name in
                tuckService.selectedDrive = tuckService.drives.first { $0.name == name }
                tuckService.loadEntries()
            }
        )) {
            ForEach(tuckService.drives, id: \.name) { drive in
                Text(drive.name).tag(drive.name)
            }
        }
        .labelsHidden()
    }
}
