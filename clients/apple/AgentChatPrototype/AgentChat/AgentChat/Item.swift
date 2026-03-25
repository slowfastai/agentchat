//
//  Item.swift
//  AgentChat
//
//  Created by Jia Li on 2026/3/24.
//

import Foundation

struct Item: Identifiable, Hashable {
    let id: UUID
    var timestamp: Date
    var title: String
    var summary: String
    var isFavorite: Bool

    init(
        id: UUID = UUID(),
        timestamp: Date,
        title: String = "",
        summary: String = "",
        isFavorite: Bool = false
    ) {
        self.id = id
        self.timestamp = timestamp
        self.title = title
        self.summary = summary
        self.isFavorite = isFavorite
    }
}
