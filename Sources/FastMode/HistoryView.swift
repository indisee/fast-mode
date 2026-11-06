import SwiftUI

struct HistoryView: View {
    @ObservedObject var model: AppModel

    var body: some View {
        if model.history.isEmpty {
            Text("No history yet")
                .font(.system(size: 12))
                .foregroundStyle(.tertiary)
                .frame(maxWidth: .infinity)
                .padding(.vertical, 12)
        } else {
            ScrollView {
                LazyVStack(spacing: 8) {
                    ForEach(model.history) { item in
                        HistoryRow(item: item) { model.delete(item) }
                    }
                }
            }
            .scrollIndicators(.automatic)
            .frame(height: 300)
        }
    }
}

struct HistoryRow: View {
    let item: HistoryItem
    let onDelete: () -> Void
    @State private var expanded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(item.date.formatted(date: .abbreviated, time: .shortened))
                    .font(.system(size: 10))
                    .foregroundStyle(.tertiary)
                Spacer()
                Button(action: onDelete) {
                    Image(systemName: "trash").font(.system(size: 10))
                }
                .buttonStyle(.plain)
                .foregroundStyle(.tertiary)
                .help("Delete")
            }
            Section(title: "Request", text: item.request, lines: expanded ? nil : 2, dim: true)
            Divider()
            Section(title: "Response", text: item.response, lines: expanded ? nil : 4, dim: false)
        }
        .padding(10)
        .background(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .fill(Color.primary.opacity(0.035))
        )
        .contentShape(Rectangle())
        .onTapGesture { withAnimation(.easeInOut(duration: 0.12)) { expanded.toggle() } }
    }

    private struct Section: View {
        let title: String
        let text: String
        let lines: Int?
        let dim: Bool
        @State private var copied = false

        var body: some View {
            HStack(alignment: .top, spacing: 8) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(title.uppercased())
                        .font(.system(size: 9, weight: .semibold))
                        .foregroundStyle(.tertiary)
                    Text(text)
                        .font(.system(size: 12))
                        .foregroundStyle(dim ? .secondary : .primary)
                        .lineLimit(lines)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                Button {
                    Pasteboard.copy(text)
                    copied = true
                    Task {
                        try? await Task.sleep(for: .seconds(1.2))
                        copied = false
                    }
                } label: {
                    Image(systemName: copied ? "checkmark" : "doc.on.doc")
                        .font(.system(size: 12))
                        .foregroundStyle(copied ? .green : .secondary)
                        .frame(width: 22, height: 22)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help("Copy \(title.lowercased())")
            }
        }
    }
}
