//
//  Item.swift
//  AgentChat
//
//  Created by Jia Li on 2026/3/24.
//

import Foundation
import SwiftData

@Model
final class Item {
    var timestamp: Date
    var title: String
    var summary: String
    var isFavorite: Bool

    init(
        timestamp: Date,
        title: String = "",
        summary: String = "",
        isFavorite: Bool = false
    ) {
        self.timestamp = timestamp
        self.title = title
        self.summary = summary
        self.isFavorite = isFavorite
    }
}
