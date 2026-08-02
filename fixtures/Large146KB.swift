import AppKit
import CoreText
import ImageIO
import SwiftUI

nonisolated struct MarkdownPreviewJumpRequest: Equatable, Sendable {
    let outlineID: String
    let generation: Int
}

struct MarkdownAttributedHeading: Equatable {
    let id: String
    let range: NSRange
}

struct MarkdownCodeHighlightRequest: Equatable {
    let range: NSRange
    let source: String
    let languageName: String?
}

struct MarkdownCodeBlockRegion: Equatable {
    let id: String
    let headerRange: NSRange
    let sourceRange: NSRange
    let source: String
    let usesGeneratedLineNumbers: Bool
}

struct MarkdownImageFigureRegion {
    let id: String
    let url: URL
    let attachment: NSTextAttachment
    let range: NSRange
}

struct MarkdownMermaidFigureRegion {
    let id: String
    let source: String
    let attachment: NSTextAttachment
    let range: NSRange
}

struct MarkdownAttributedDocument {
    let attributedString: NSAttributedString
    let headings: [MarkdownAttributedHeading]
    let codeHighlights: [MarkdownCodeHighlightRequest]
    let codeBlocks: [MarkdownCodeBlockRegion]
    let imageFigures: [MarkdownImageFigureRegion]
    let mermaidFigures: [MarkdownMermaidFigureRegion]

    static let empty = MarkdownAttributedDocument(
        attributedString: NSAttributedString(),
        headings: [],
        codeHighlights: [],
        codeBlocks: [],
        imageFigures: [],
        mermaidFigures: []
    )
}

extension NSAttributedString.Key {
    /// Marks a block that fills the whole container instead of the prose measure.
    nonisolated static let atelierBleedBlock = NSAttributedString.Key(
        "app.atelier.markdown.bleed-block"
    )
    nonisolated static let atelierBlockquoteBar = NSAttributedString.Key(
        "app.atelier.markdown.blockquote-bar"
    )
    /// Heading level (1 or 2). Drives the drawn accent + hairline rule.
    nonisolated static let atelierHeadingRule = NSAttributedString.Key(
        "app.atelier.markdown.heading-rule"
    )
    nonisolated static let atelierCodeLineNumber = NSAttributedString.Key(
        "app.atelier.markdown.code-line-number"
    )
}

@MainActor
private final class MarkdownCodeLineNumberDecoration: NSObject {
    let line: CTLine
    let width: CGFloat

    @MainActor
    init(number: Int, codeFont: NSFont) {
        let text = NSAttributedString(
            string: String(number),
            attributes: [
                .font: AtelierTypography.codeFont(size: codeFont.pointSize * MarkdownTypeTokens.FontScale.codeLineNumber),
                .foregroundColor: AppKitThemeAdapter.secondary.withAlphaComponent(0.55)
            ]
        )
        let line = CTLineCreateWithAttributedString(text)
        self.line = line
        self.width = CGFloat(CTLineGetTypographicBounds(line, nil, nil, nil))
        super.init()
    }
}

private enum MarkdownHeadingRuleLayout {
    static let thickness: CGFloat = 1.5
    static let primaryLeadWidth = AtelierMetrics.space2XL
    static let secondaryLeadWidth = AtelierMetrics.spaceL
}

/// Inline code is set slightly smaller than the prose around it: a monospaced
/// face at the same point size reads larger than the sans it sits in.
nonisolated enum MarkdownInlineCodePolicy {
    static let fontScale = MarkdownTypeTokens.FontScale.inlineCode
}

nonisolated enum MarkdownCodeCardLayout {
    static let bodyTintAlpha: CGFloat = 0.40
    static let copyControlReservation: CGFloat = 44
}

/// One place to tune the Markdown reading type scale. These are the body-relative
/// font-size multipliers and the heading size ratios that the block builders read.
/// Line-height ratios live in `MarkdownTypeScale` and block spacing in
/// `MarkdownRhythm`: those stay their own cohesive types, but every raw size ratio
/// the document renders with resolves back to a name here.
nonisolated enum MarkdownTypeTokens {
    /// Body-relative font-size multipliers, one per block treatment that sets text
    /// smaller or larger than the surrounding prose.
    enum FontScale {
        static let lede: CGFloat = 1.14
        static let inlineCode: CGFloat = 0.92
        static let tableCell: CGFloat = 0.9
        static let frontMatterKey: CGFloat = 0.68
        static let frontMatterValue: CGFloat = 0.84
        static let calloutLabel: CGFloat = 0.68
        static let mermaidSourceLabel: CGFloat = 0.8
        static let caption: CGFloat = 0.82
        static let footnotesTitle: CGFloat = 0.82
        static let footnoteNumber: CGFloat = 0.72
        static let footnoteText: CGFloat = 0.86
        static let divider: CGFloat = 0.55
        static let codeLineNumber: CGFloat = 0.85
    }

    /// Heading size as a pure ratio of body size, never a minimum clamp: a floor
    /// changes the hierarchy's shape as the reader resizes text or zooms.
    static func headingRatio(level: Int, isDocument: Bool) -> CGFloat {
        switch level {
        case 1: isDocument ? 1.85 : 1.45
        case 2: isDocument ? 1.45 : 1.28
        case 3: isDocument ? 1.18 : 1.12
        case 4: 1.00
        default: 0.92
        }
    }
}

/// Line spacing as a target line-height ratio instead of an absolute value.
/// `NSParagraphStyle.lineSpacing` adds room on top of the font's own line height,
/// so one fixed point value is a big share of the total at 10 points and a small
/// share at 32. Back-solving from a ratio holds the same reading rhythm across the
/// whole text-scale and zoom range.
nonisolated struct MarkdownTypeScale: Equatable, Sendable {
    static let prose: CGFloat = 1.62
    static let lede: CGFloat = 1.70
    static let list: CGFloat = 1.55
    static let tableCell: CGFloat = 1.45
    static let code: CGFloat = 1.35
    static let displayHeading: CGFloat = 1.16
    static let heading: CGFloat = 1.30

    let displayScale: CGFloat

    static func lineHeight(of font: NSFont) -> CGFloat {
        ceil(font.ascender - font.descender + font.leading)
    }

    @MainActor
    func lineSpacing(for font: NSFont, ratio: CGFloat) -> CGFloat {
        let extra = font.pointSize * ratio - Self.lineHeight(of: font)
        guard extra > 0 else { return 0 }
        return AtelierFontScaling.snapped(extra, displayScale: displayScale)
    }
}

nonisolated struct MarkdownRhythm: Equatable, Sendable {
    let unit: CGFloat
    let type: MarkdownTypeScale

    @MainActor
    init(bodyFont: NSFont, displayScale: CGFloat) {
        unit = AtelierFontScaling.snapped(
            MarkdownTypeScale.lineHeight(of: bodyFont),
            displayScale: displayScale
        )
        type = MarkdownTypeScale(displayScale: displayScale)
    }

    /// Three weight tiers. A heavy block has to separate more than a paragraph
    /// does, or a table reads as one more run of prose.
    /// Flow: paragraph, list item, lede.
    var flow: CGFloat { unit * 0.5 }
    /// Structure: code card, table, figure, Mermaid, callout, quote, front matter, notes.
    var structure: CGFloat { unit * 1.25 }
    /// Break: divider, H1, H2.
    var breakBefore: CGFloat { unit * 1.75 }
    var breakAfter: CGFloat { unit * 0.6 }

    var paragraph: CGFloat { flow }
    /// Items inside one list share the flow gap, so each edge carries half of it.
    var listItem: CGFloat { flow * 0.5 }
    var heading3Before: CGFloat { unit }
    var heading3After: CGFloat { unit * 0.5 }
    var headingPrimaryBefore: CGFloat { breakBefore }
    var headingPrimaryAfter: CGFloat { breakAfter }
    var codeCard: CGFloat { structure }
    var lede: CGFloat { unit * 0.75 }
}

/// Fits decoded figure content to the prose measure without letterboxing it.
nonisolated enum MarkdownFigureFitPolicy {
    static func fittedSize(
        contentWidth: CGFloat,
        contentHeight: CGFloat,
        measure: CGFloat,
        maximumHeight: CGFloat
    ) -> NSSize {
        guard contentWidth > 0, contentHeight > 0, measure > 0 else {
            return NSSize(width: max(1, measure), height: max(1, maximumHeight))
        }
        var width = min(measure, contentWidth)
        var height = width * contentHeight / contentWidth
        if maximumHeight > 0, height > maximumHeight {
            height = maximumHeight
            width = height * contentWidth / contentHeight
        }
        return NSSize(
            width: max(1, width.rounded()),
            height: max(1, height.rounded())
        )
    }
}

nonisolated enum MarkdownImageFigureLayout {
    /// Placeholder shape only. A decoded image adopts its own aspect ratio.
    static let aspectRatio: CGFloat = 16 / 9
    static let maximumHeightScale: CGFloat = 1.2

    @MainActor
    static func reservedBounds(
        measure: CGFloat = AtelierMetrics.documentMaxWidth
    ) -> NSRect {
        let width = max(1, measure)
        return NSRect(
            x: 0,
            y: 0,
            width: width,
            height: (width / aspectRatio).rounded()
        )
    }

    @MainActor
    static func fittedBounds(
        pixelWidth: Int,
        pixelHeight: Int,
        measure: CGFloat = AtelierMetrics.documentMaxWidth
    ) -> NSRect {
        let measure = max(1, measure)
        let size = MarkdownFigureFitPolicy.fittedSize(
            contentWidth: CGFloat(pixelWidth),
            contentHeight: CGFloat(pixelHeight),
            measure: measure,
            maximumHeight: measure * maximumHeightScale
        )
        return NSRect(origin: .zero, size: size)
    }
}

nonisolated enum MarkdownMermaidFigureLayout {
    static let placeholderAspectRatio: CGFloat = 16 / 9
    static let maximumHeightScale: CGFloat = 1.4
    static let loadingMessage = "Rendering Mermaid diagram..."
    static let failureMessage = "Mermaid diagram could not be rendered."

    /// Trailing room for the source toggle, so the control never sits on the
    /// diagram. The figure paragraph reserves the same width as a tail indent.
    static func contentMeasure(_ measure: CGFloat) -> CGFloat {
        max(1, measure - MarkdownCodeCardLayout.copyControlReservation)
    }

    @MainActor
    static func renderWidth(
        measure: CGFloat = AtelierMetrics.documentMaxWidth
    ) -> CGFloat {
        MermaidRenderingPolicy.widthBucket(
            containerWidth: contentMeasure(measure)
        )
    }

    @MainActor
    static func reservedBounds(
        measure: CGFloat = AtelierMetrics.documentMaxWidth
    ) -> NSRect {
        let width = contentMeasure(measure)
        return NSRect(
            x: 0,
            y: 0,
            width: width,
            height: (width / placeholderAspectRatio).rounded()
        )
    }

    @MainActor
    static func fittedBounds(
        imageSize: NSSize,
        measure: CGFloat = AtelierMetrics.documentMaxWidth
    ) -> NSRect {
        let measure = contentMeasure(measure)
        let size = MarkdownFigureFitPolicy.fittedSize(
            contentWidth: imageSize.width,
            contentHeight: imageSize.height,
            measure: measure,
            maximumHeight: measure * maximumHeightScale
        )
        return NSRect(origin: .zero, size: size)
    }
}

nonisolated struct MarkdownFrontMatterMastheadPlan: Equatable, Sendable {
    let headingIndex: Int
    let ledeIndex: Int?
    let masthead: [MarkdownFrontMatterEntry]
    let remaining: [MarkdownFrontMatterEntry]
}

nonisolated enum MarkdownFrontMatterMastheadPolicy {
    static let maximumEntryCount = 3
    static let maximumValueLength = 48

    static func plan(
        blocks: [AgentMarkdownBlock]
    ) -> MarkdownFrontMatterMastheadPlan? {
        guard blocks.count > 1,
              case .frontMatter(let entries) = blocks[0],
              case .heading(let level, _) = blocks[1],
              level == 1 else {
            return nil
        }
        let eligible = entries.filter {
            $0.key.caseInsensitiveCompare("title") != .orderedSame
                && !$0.value.isEmpty
                && $0.value.count <= maximumValueLength
                && !$0.value.contains(",")
        }
        let masthead = Array(eligible.prefix(maximumEntryCount))
        let selectedKeys = Set(masthead.map(\.key))
        let remaining = entries.filter { !selectedKeys.contains($0.key) }
        let ledeIndex: Int? = blocks.indices.contains(2) && {
            if case .paragraph = blocks[2] { return true }
            return false
        }() ? 2 : nil
        return MarkdownFrontMatterMastheadPlan(
            headingIndex: 1,
            ledeIndex: ledeIndex,
            masthead: masthead,
            remaining: remaining
        )
    }
}

nonisolated enum MarkdownReadingProgressPolicy {
    static func fraction(
        originY: CGFloat,
        viewportHeight: CGFloat,
        documentHeight: CGFloat
    ) -> CGFloat {
        let maximum = max(0, documentHeight - max(0, viewportHeight))
        guard maximum > 0 else { return 0 }
        return min(1, max(0, originY / maximum))
    }

    static func visiblePixel(
        fraction: CGFloat,
        railHeight: CGFloat
    ) -> Int {
        Int((min(1, max(0, fraction)) * max(0, railHeight)).rounded())
    }
}

nonisolated enum MarkdownLinkStylePolicy {
    static let normalUnderlineAlpha: CGFloat = 0.35

    @MainActor
    static var normalUnderlineColor: NSColor {
        AppKitThemeAdapter.accent.withAlphaComponent(normalUnderlineAlpha)
    }
}

/// Editorial type decisions taken once during document construction.
nonisolated enum MarkdownDocumentTypePolicy {
    static let ledeScale = MarkdownTypeTokens.FontScale.lede

    /// A document that opens with an H1 gives its first paragraph the lede treatment.
    static func hasLedeParagraph(blocks: [AgentMarkdownBlock]) -> Bool {
        if blocks.count > 1,
           case .heading(let level, _) = blocks[0],
           level == 1,
           case .paragraph = blocks[1] {
            return true
        }
        return MarkdownFrontMatterMastheadPolicy.plan(blocks: blocks)?.ledeIndex != nil
    }
}

nonisolated enum MarkdownTableCellPolicy {
    static let fontScale = MarkdownTypeTokens.FontScale.tableCell
    static let unbrokenTokenLimit = 24

    static func wrapsByCharacter(_ value: String) -> Bool {
        var tokenLength = 0
        for character in value {
            if character.isWhitespace {
                tokenLength = 0
                continue
            }
            tokenLength += 1
            if tokenLength > unbrokenTokenLimit { return true }
        }
        return false
    }
}

/// Draw-time geometry resolved once on the main actor and captured by the layout
/// manager. Keeps the draw path free of theme lookups and allocations.
nonisolated struct MarkdownPreviewDecorationMetrics: Sendable {
    let headingRuleThickness: CGFloat
    let headingRulePrimaryLead: CGFloat
    let headingRuleSecondaryLead: CGFloat
    let hairline: CGFloat
    let quoteGlyphFontSize: CGFloat
    let quoteGlyphAlpha: CGFloat
    let codeLineNumberGap: CGFloat

    @MainActor
    static var current: Self {
        Self(
            headingRuleThickness: MarkdownHeadingRuleLayout.thickness,
            headingRulePrimaryLead: MarkdownHeadingRuleLayout.primaryLeadWidth,
            headingRuleSecondaryLead: MarkdownHeadingRuleLayout.secondaryLeadWidth,
            hairline: AtelierTheme.strokeHairline,
            quoteGlyphFontSize: 38,
            quoteGlyphAlpha: 0.18,
            codeLineNumberGap: AtelierMetrics.spaceS
        )
    }
}

nonisolated
final class MarkdownPreviewLayoutManager: NSLayoutManager {
    private let metrics: MarkdownPreviewDecorationMetrics
    private let quoteFont: CTFont
    private let quoteGlyph: CGGlyph
    private let quoteColor: CGColor

    init(metrics: MarkdownPreviewDecorationMetrics) {
        self.metrics = metrics
        let quoteFont = CTFontCreateWithName(
            "NewYork" as CFString,
            metrics.quoteGlyphFontSize,
            nil
        )
        self.quoteFont = quoteFont
        var character = Array("\u{201C}".utf16)
        var glyph = CGGlyph()
        if !CTFontGetGlyphsForCharacters(
            quoteFont,
            &character,
            &glyph,
            character.count
        ) {
            glyph = 0
        }
        self.quoteGlyph = glyph
        self.quoteColor = AppKitThemeAdapter.accent
            .withAlphaComponent(metrics.quoteGlyphAlpha)
            .cgColor
        super.init()
    }

    required init?(coder: NSCoder) {
        return nil
    }

    override func drawBackground(forGlyphRange glyphsToShow: NSRange, at origin: NSPoint) {
        super.drawBackground(forGlyphRange: glyphsToShow, at: origin)
        guard let textStorage else { return }
        let characterRange = characterRange(
            forGlyphRange: glyphsToShow,
            actualGlyphRange: nil
        )
        textStorage.enumerateAttribute(
            .atelierBlockquoteBar,
            in: characterRange
        ) { value, range, _ in
            guard let color = value as? NSColor else { return }
            let barGlyphRange = glyphRange(
                forCharacterRange: range,
                actualCharacterRange: nil
            )
            // One fill for the whole visible run: per-fragment fills band on soft wrap.
            var minY = CGFloat.greatestFiniteMagnitude
            var maxY = -CGFloat.greatestFiniteMagnitude
            enumerateLineFragments(forGlyphRange: barGlyphRange) {
                lineRect, _, _, lineGlyphRange, _ in
                let visibleRange = NSIntersectionRange(
                    NSIntersectionRange(barGlyphRange, lineGlyphRange),
                    glyphsToShow
                )
                guard visibleRange.length > 0 else { return }
                minY = min(minY, lineRect.minY)
                maxY = max(maxY, lineRect.maxY)
            }
            guard maxY > minY else { return }
            let quoteStyle = textStorage.attribute(
                .paragraphStyle,
                at: range.location,
                effectiveRange: nil
            ) as? NSParagraphStyle
            let quoteInset = max(
                0,
                (quoteStyle?.headIndent ?? 0) - MarkdownQuoteLayout.indent
            )
            color.setFill()
            NSRect(
                x: origin.x + quoteInset + MarkdownQuoteLayout.leadingInset,
                y: minY + origin.y,
                width: MarkdownQuoteLayout.barWidth,
                height: maxY - minY
            ).fill()
            guard self.quoteGlyph != 0,
                  NSLocationInRange(barGlyphRange.location, glyphsToShow),
                  let context = NSGraphicsContext.current?.cgContext else {
                return
            }
            var glyph = self.quoteGlyph
            var position = CGPoint(
                x: origin.x + quoteInset + MarkdownQuoteLayout.leadingInset
                    + MarkdownQuoteLayout.barWidth
                    + 4,
                y: minY + origin.y + self.metrics.quoteGlyphFontSize * 0.78
            )
            context.saveGState()
            context.setFillColor(self.quoteColor)
            CTFontDrawGlyphs(
                self.quoteFont,
                &glyph,
                &position,
                1,
                context
            )
            context.restoreGState()
        }
        textStorage.enumerateAttribute(
            .atelierHeadingRule,
            in: characterRange
        ) { value, range, _ in
            guard let level = (value as? NSNumber)?.intValue else { return }
            drawHeadingRule(level: level, characterRange: range, at: origin)
        }
        textStorage.enumerateAttribute(
            .atelierCodeLineNumber,
            in: characterRange
        ) { value, range, _ in
            guard let decoration = value as? MarkdownCodeLineNumberDecoration,
                  let graphicsContext = NSGraphicsContext.current else {
                return
            }
            let context = graphicsContext.cgContext
            let isFlipped = graphicsContext.isFlipped
            let anchorGlyphRange = glyphRange(
                forCharacterRange: range,
                actualCharacterRange: nil
            )
            let visibleAnchor = NSIntersectionRange(
                anchorGlyphRange,
                glyphsToShow
            )
            guard visibleAnchor.length > 0 else { return }
            let glyphIndex = visibleAnchor.location
            var fragment = NSRect.zero
            var usedFragment = NSRect.zero
            enumerateLineFragments(
                forGlyphRange: NSRange(location: glyphIndex, length: 1)
            ) { lineRect, usedRect, _, _, stop in
                fragment = lineRect
                usedFragment = usedRect
                stop.pointee = true
            }
            let baseline = fragment.minY + location(forGlyphAt: glyphIndex).y
            let textPosition = CGPoint(
                x: origin.x
                    + usedFragment.minX
                    - metrics.codeLineNumberGap,
                y: origin.y + baseline
            )
            MainActor.assumeIsolated {
                let previousTextMatrix = context.textMatrix
                context.saveGState()
                context.textMatrix = isFlipped
                    ? CGAffineTransform(scaleX: 1, y: -1)
                    : .identity
                context.textPosition = CGPoint(
                    x: textPosition.x - decoration.width,
                    y: textPosition.y
                )
                CTLineDraw(decoration.line, context)
                context.textMatrix = previousTextMatrix
                context.restoreGState()
            }
        }
    }

    /// Accent lead segment plus a hairline across the measure, aligned to the bottom
    /// of the heading's last line fragment. Colors are precomputed theme constants.
    private func drawHeadingRule(level: Int, characterRange: NSRange, at origin: NSPoint) {
        let headingGlyphRange = glyphRange(
            forCharacterRange: characterRange,
            actualCharacterRange: nil
        )
        var lastRect: NSRect?
        var measureWidth: CGFloat = 0
        enumerateLineFragments(forGlyphRange: headingGlyphRange) {
            lineRect, _, textContainer, _, _ in
            lastRect = lineRect
            measureWidth = textContainer.size.width - textContainer.lineFragmentPadding * 2
        }
        guard let lastRect, measureWidth > 0 else { return }
        // The container can be wider than the prose it holds, so follow the
        // paragraph's own indents. Reading an attribute allocates nothing.
        let style = textStorage?.attribute(
            .paragraphStyle,
            at: characterRange.location,
            effectiveRange: nil
        ) as? NSParagraphStyle
        let leading = max(0, style?.headIndent ?? 0)
        let trailing = max(0, -(style?.tailIndent ?? 0))
        let ruleWidth = measureWidth - leading - trailing
        guard ruleWidth > 0 else { return }
        let thickness = metrics.headingRuleThickness
        let leadWidth = min(
            ruleWidth,
            level == 1
                ? metrics.headingRulePrimaryLead
                : metrics.headingRuleSecondaryLead
        )
        let y = (lastRect.maxY + origin.y - thickness).rounded(.down)
        let x = origin.x + lastRect.minX + leading
        AppKitThemeAdapter.accent.setFill()
        NSRect(x: x, y: y, width: leadWidth, height: thickness).fill()
        guard ruleWidth > leadWidth else { return }
        AppKitThemeAdapter.border.setFill()
        NSRect(
            x: x + leadWidth,
            y: y + (thickness - metrics.hairline) / 2,
            width: ruleWidth - leadWidth,
            height: metrics.hairline
        ).fill()
    }
}

@MainActor
enum MarkdownAttributedDocumentBuilder {
    private static let colorSwatchExpression = try? NSRegularExpression(
        pattern: "\u{25A0}(#(?:[0-9A-Fa-f]{8}|[0-9A-Fa-f]{6}|[0-9A-Fa-f]{4}|[0-9A-Fa-f]{3}))"
    )

    static func build(
        document: ParsedMarkdownDocument,
        scale: CGFloat,
        displayScale: CGFloat,
        usesDarkAppearance: Bool,
        presentation: AgentMarkdownPresentation = .document,
        containerMeasure: CGFloat? = nil
    ) -> MarkdownAttributedDocument {
        let output = NSMutableAttributedString()
        var headings: [MarkdownAttributedHeading] = []
        var codeHighlights: [MarkdownCodeHighlightRequest] = []
        var codeBlocks: [MarkdownCodeBlockRegion] = []
        var imageFigures: [MarkdownImageFigureRegion] = []
        var mermaidFigures: [MarkdownMermaidFigureRegion] = []
        let isDocument = presentation == .document
        let bodySize = AtelierFontScaling.snapped(
            (isDocument ? AtelierTypography.editorSize : AtelierTypography.body)
                * scale,
            displayScale: displayScale
        )
        let bodyFont = documentBodyFont(size: bodySize, presentation: presentation)
        let codeFont = codeFont(
            scale: scale,
            displayScale: displayScale,
            presentation: presentation
        )
        // Two measures in document mode: wide blocks fill the container, prose is
        // held narrower by an inset applied after the document is built.
        let measure = MarkdownBleedPolicy.containerMeasure(
            requested: containerMeasure,
            presentation: presentation
        )
        let proseMeasure = MarkdownBleedPolicy.proseMeasure(
            containerMeasure: measure,
            presentation: presentation
        )
        let proseInset = MarkdownBleedPolicy.proseInset(
            containerMeasure: measure,
            proseMeasure: proseMeasure
        )
        let rhythm = MarkdownRhythm(
            bodyFont: bodyFont,
            displayScale: displayScale
        )

        // Lede scale, front-matter masthead, and the H3 accent eyebrow are
        // document-only treatments. Transcript mode keeps every other one.
        let hasLede = isDocument && MarkdownDocumentTypePolicy.hasLedeParagraph(
            blocks: document.blocks
        )
        let mastheadPlan = isDocument
            ? MarkdownFrontMatterMastheadPolicy.plan(blocks: document.blocks)
            : nil
        let footnoteNumbers = MarkdownFootnotePolicy.numberMap(
            in: document.blocks
        )

        for (index, block) in document.blocks.enumerated() {
            switch block {
            case .frontMatter(let entries):
                if mastheadPlan != nil, index == 0 {
                    continue
                }
                appendFrontMatter(
                    entries,
                    to: output,
                    bodySize: bodySize,
                    codeFont: codeFont,
                    rhythm: rhythm,
                    measure: measure
                )

            case .heading(let level, let content):
                let start = output.length
                let font = headingFont(
                    level: level,
                    bodySize: bodySize,
                    presentation: presentation
                )
                let paragraph = paragraphStyle(
                    lineSpacing: rhythm.type.lineSpacing(
                        for: font,
                        ratio: level <= 2
                            ? MarkdownTypeScale.displayHeading
                            : MarkdownTypeScale.heading
                    ),
                    before: output.length == 0 ? 0 : headingTopSpacing(
                        level: level,
                        rhythm: rhythm
                    ),
                    after: level <= 2
                        ? rhythm.headingPrimaryAfter
                        : rhythm.heading3After
                )
                let text = inlineText(
                    content,
                    font: font,
                    foregroundColor: headingColor(
                        level: level,
                        presentation: presentation
                    ),
                    paragraphStyle: paragraph,
                    codeFont: codeFont,
                    footnoteNumbers: footnoteNumbers
                )
                let headingRange = NSRange(location: 0, length: text.length)
                if level <= 2 {
                    // Rule is drawn in the layout pass; a glyph underline would
                    // stop at the text width and clip on wrapped headings.
                    text.addAttributes([
                        .kern: -0.5,
                        .atelierHeadingRule: NSNumber(value: level)
                    ], range: headingRange)
                } else if level == 3 || level >= 5 {
                    text.addAttribute(.kern, value: 0.6, range: headingRange)
                }
                output.append(text)
                let headingLength = text.length
                output.append(NSAttributedString(string: "\n", attributes: [
                    .font: font,
                    .paragraphStyle: paragraph
                ]))

                let title = content.trimmingCharacters(in: .whitespacesAndNewlines)
                if !title.isEmpty {
                    headings.append(
                        MarkdownAttributedHeading(
                            id: AgentMarkdownBlock.blockAnchorID(index),
                            range: NSRange(location: start, length: max(1, headingLength))
                        )
                    )
                }
                if let mastheadPlan,
                   index == mastheadPlan.headingIndex {
                    appendMasthead(
                        mastheadPlan.masthead,
                        to: output,
                        bodySize: bodySize,
                        rhythm: rhythm
                    )
                    if mastheadPlan.ledeIndex == nil {
                        appendFrontMatter(
                            mastheadPlan.remaining,
                            to: output,
                            bodySize: bodySize,
                            codeFont: codeFont,
                            rhythm: rhythm,
                            measure: measure
                        )
                    }
                }

            case .paragraph(let content):
                let isLede = hasLede
                    && (index == 1 || index == mastheadPlan?.ledeIndex)
                let ledeFont = isLede
                    ? serifFont(
                        size: AtelierFontScaling.snapped(
                            bodySize * MarkdownDocumentTypePolicy.ledeScale,
                            displayScale: displayScale
                        ),
                        weight: .regular
                    )
                    : bodyFont
                appendInlineParagraph(
                    content,
                    to: output,
                    font: ledeFont,
                    codeFont: codeFont,
                    paragraphStyle: paragraphStyle(
                        lineSpacing: rhythm.type.lineSpacing(
                            for: ledeFont,
                            ratio: isLede
                                ? MarkdownTypeScale.lede
                                : MarkdownTypeScale.prose
                        ),
                        before: output.length == 0 ? 0 : rhythm.paragraph,
                        after: isLede ? rhythm.lede : rhythm.paragraph
                    ),
                    footnoteNumbers: footnoteNumbers
                )
                if let mastheadPlan,
                   index == mastheadPlan.ledeIndex {
                    appendFrontMatter(
                        mastheadPlan.remaining,
                        to: output,
                        bodySize: bodySize,
                        codeFont: codeFont,
                        rhythm: rhythm,
                        measure: measure
                    )
                }

            case .unorderedItem(let depth, let content):
                appendListItem(
                    marker: unorderedMarker(depth: depth),
                    depth: depth,
                    content: content,
                    to: output,
                    bodyFont: bodyFont,
                    codeFont: codeFont,
                    rhythm: rhythm,
                    footnoteNumbers: footnoteNumbers
                )

            case .orderedItem(let number, let depth, let content):
                appendListItem(
                    marker: "\(number).",
                    depth: depth,
                    content: content,
                    to: output,
                    bodyFont: bodyFont,
                    codeFont: codeFont,
                    rhythm: rhythm,
                    footnoteNumbers: footnoteNumbers
                )

            case .taskItem(let isCompleted, let depth, let content):
                appendListItem(
                    marker: isCompleted ? "\u{2611}" : "\u{2610}",
                    depth: depth,
                    content: content,
                    to: output,
                    bodyFont: bodyFont,
                    codeFont: codeFont,
                    isCompleted: isCompleted,
                    rhythm: rhythm,
                    footnoteNumbers: footnoteNumbers
                )

            case .quote(let content):
                let quoteFont = serifItalicFont(size: bodySize)
                let paragraph = paragraphStyle(
                    lineSpacing: rhythm.type.lineSpacing(
                        for: quoteFont,
                        ratio: MarkdownTypeScale.prose
                    ),
                    before: rhythm.structure,
                    after: rhythm.structure,
                    firstLineHeadIndent: MarkdownQuoteLayout.indent,
                    headIndent: MarkdownQuoteLayout.indent,
                    tailIndent: -AtelierMetrics.spaceL
                )
                let quoteStart = output.length
                let quote = inlineText(
                    content,
                    font: quoteFont,
                    foregroundColor: AppKitThemeAdapter.secondary,
                    paragraphStyle: paragraph,
                    codeFont: codeFont,
                    footnoteNumbers: footnoteNumbers
                )
                output.append(quote)
                output.append(NSAttributedString(string: "\n", attributes: [
                    .font: bodyFont,
                    .paragraphStyle: paragraph
                ]))
                output.addAttribute(
                    .atelierBlockquoteBar,
                    value: AppKitThemeAdapter.accent,
                    range: NSRange(
                        location: quoteStart,
                        length: output.length - quoteStart
                    )
                )

            case .callout(let kind, let content):
                appendCallout(
                    kind: kind,
                    content: content,
                    to: output,
                    bodyFont: bodyFont,
                    codeFont: codeFont,
                    rhythm: rhythm,
                    footnoteNumbers: footnoteNumbers
                )

            case .code(let language, let content):
                appendCode(
                    id: AgentMarkdownBlock.blockAnchorID(index),
                    language: language,
                    content: content,
                    presentation: presentation,
                    to: output,
                    codeFont: codeFont,
                    rhythm: rhythm,
                    codeHighlights: &codeHighlights,
                    codeBlocks: &codeBlocks
                )

            case .mermaid(let source):
                appendMermaidFigure(
                    id: AgentMarkdownBlock.blockAnchorID(index),
                    source: source,
                    to: output,
                    bodyFont: bodyFont,
                    rhythm: rhythm,
                    measure: measure,
                    mermaidFigures: &mermaidFigures
                )

            case .invalidMermaid(let source, let error):
                appendMermaid(
                    source: source,
                    error: error,
                    to: output,
                    bodyFont: bodyFont,
                    codeFont: codeFont,
                    rhythm: rhythm
                )

            case .table(let headers, let alignments, let rows):
                appendTable(
                    headers: headers,
                    alignments: alignments,
                    rows: rows,
                    to: output,
                    bodyFont: bodyFont,
                    codeFont: codeFont,
                    usesDarkAppearance: usesDarkAppearance,
                    rhythm: rhythm,
                    footnoteNumbers: footnoteNumbers
                )

            case .image(let altText, let urlText):
                appendImageFigure(
                    id: AgentMarkdownBlock.blockAnchorID(index),
                    altText: altText,
                    urlText: urlText,
                    sourceDirectoryURL: document.sourceDirectoryURL,
                    to: output,
                    bodyFont: bodyFont,
                    rhythm: rhythm,
                    measure: measure,
                    imageFigures: &imageFigures
                )

            case .footnotes(let notes):
                appendFootnotes(
                    notes,
                    to: output,
                    bodyFont: bodyFont,
                    codeFont: codeFont,
                    rhythm: rhythm
                )

            case .divider:
                let paragraph = paragraphStyle(
                    lineSpacing: 0,
                    before: rhythm.breakBefore,
                    after: rhythm.breakAfter
                )
                paragraph.alignment = .center
                let ornament = NSMutableAttributedString(
                    string: "\u{2022}\u{2003}\u{2022}\u{2003}\u{2022}\n",
                    attributes: [
                        .font: NSFont.systemFont(
                            ofSize: max(6, bodySize * MarkdownTypeTokens.FontScale.divider),
                            weight: .semibold
                        ),
                        .foregroundColor: AppKitThemeAdapter.border,
                        .paragraphStyle: paragraph
                    ]
                )
                ornament.addAttribute(
                    .foregroundColor,
                    value: AppKitThemeAdapter.accent,
                    range: NSRange(location: 2, length: 1)
                )
                output.append(ornament)
            }
        }

        applyProseInset(proseInset, to: output)

        return MarkdownAttributedDocument(
            attributedString: NSAttributedString(attributedString: output),
            headings: headings,
            codeHighlights: codeHighlights,
            codeBlocks: codeBlocks,
            imageFigures: imageFigures,
            mermaidFigures: mermaidFigures
        )
    }

    /// Code must stay just under the mode's prose size; one fixed size would
    /// read larger than transcript body text.
    static func codeFont(
        scale: CGFloat,
        displayScale: CGFloat,
        presentation: AgentMarkdownPresentation
    ) -> NSFont {
        AtelierTypography.codeFont(
            size: AtelierFontScaling.snapped(
                (
                    presentation == .document
                        ? AtelierTypography.uiSize
                        : AtelierTypography.label
                ) * scale,
                displayScale: displayScale
            )
        )
    }

    /// Labelled Mermaid source revealed under a rendered figure. It lives in the
    /// same text storage, so selection and `Cmd-C` still cross it.
    static func mermaidSourceBlock(
        source: String,
        scale: CGFloat,
        displayScale: CGFloat,
        presentation: AgentMarkdownPresentation
    ) -> NSAttributedString {
        let code = codeFont(
            scale: scale,
            displayScale: displayScale,
            presentation: presentation
        )
        let headerStyle = paragraphStyle(
            lineSpacing: 0,
            before: AtelierMetrics.spaceXS,
            after: AtelierMetrics.spaceXS,
            firstLineHeadIndent: AtelierMetrics.spaceM,
            headIndent: AtelierMetrics.spaceM,
            tailIndent: -AtelierMetrics.spaceM
        )
        let output = NSMutableAttributedString(
            string: "MERMAID SOURCE\n",
            attributes: [
                .font: AtelierTypography.codeFont(
                    size: max(9, code.pointSize * MarkdownTypeTokens.FontScale.mermaidSourceLabel)
                ),
                .foregroundColor: AppKitThemeAdapter.secondary,
                .backgroundColor: AppKitThemeAdapter.raised,
                .kern: 1.5,
                .paragraphStyle: headerStyle
            ]
        )
        let sourceStyle = paragraphStyle(
            lineSpacing: MarkdownTypeScale(displayScale: displayScale)
                .lineSpacing(for: code, ratio: MarkdownTypeScale.code),
            before: 0,
            after: AtelierMetrics.spaceL,
            firstLineHeadIndent: AtelierMetrics.spaceM,
            headIndent: AtelierMetrics.spaceM,
            tailIndent: -AtelierMetrics.spaceM
        )
        output.append(NSAttributedString(string: source + "\n", attributes: [
            .font: code,
            .foregroundColor: AppKitThemeAdapter.foreground,
            .backgroundColor: AppKitThemeAdapter.code,
            .paragraphStyle: sourceStyle
        ]))
        return output
    }

    private static func appendInlineParagraph(
        _ content: String,
        to output: NSMutableAttributedString,
        font: NSFont,
        codeFont: NSFont,
        paragraphStyle: NSParagraphStyle,
        footnoteNumbers: [String: Int] = [:]
    ) {
        output.append(
            inlineText(
                content,
                font: font,
                foregroundColor: AppKitThemeAdapter.foreground,
                paragraphStyle: paragraphStyle,
                codeFont: codeFont,
                footnoteNumbers: footnoteNumbers
            )
        )
        output.append(NSAttributedString(string: "\n", attributes: [
            .font: font,
            .paragraphStyle: paragraphStyle
        ]))
    }

    private static func appendListItem(
        marker: String,
        depth: Int,
        content: String,
        to output: NSMutableAttributedString,
        bodyFont: NSFont,
        codeFont: NSFont,
        isCompleted: Bool = false,
        rhythm: MarkdownRhythm,
        footnoteNumbers: [String: Int]
    ) {
        let markerWidth = AtelierMetrics.spaceXL
            + CGFloat(depth) * AtelierMetrics.spaceXL
        let paragraph = paragraphStyle(
            lineSpacing: rhythm.type.lineSpacing(
                for: bodyFont,
                ratio: MarkdownTypeScale.list
            ),
            before: rhythm.listItem,
            after: rhythm.listItem,
            firstLineHeadIndent: -1,
            headIndent: markerWidth
        )
        // One explicit stop keeps single-line and wrapped items on the same indent.
        paragraph.tabStops = [
            NSTextTab(textAlignment: .left, location: markerWidth, options: [:])
        ]
        paragraph.defaultTabInterval = markerWidth
        let markerColor = blendedColor(
            from: AppKitThemeAdapter.secondary,
            to: AppKitThemeAdapter.border,
            fraction: min(1, CGFloat(depth) / 3)
        )
        output.append(NSAttributedString(string: marker + "\t", attributes: [
            .font: NSFont.systemFont(ofSize: bodyFont.pointSize, weight: .semibold),
            .foregroundColor: markerColor,
            .paragraphStyle: paragraph
        ]))
        let text = inlineText(
            content,
            font: bodyFont,
            foregroundColor: isCompleted
                ? AppKitThemeAdapter.secondary
                : AppKitThemeAdapter.foreground,
            paragraphStyle: paragraph,
            codeFont: codeFont,
            footnoteNumbers: footnoteNumbers
        )
        if isCompleted {
            text.addAttributes([
                .strikethroughStyle: NSUnderlineStyle.single.rawValue,
                .strikethroughColor: AppKitThemeAdapter.secondary
            ], range: NSRange(location: 0, length: text.length))
        }
        output.append(text)
        output.append(NSAttributedString(string: "\n", attributes: [
            .font: bodyFont,
            .paragraphStyle: paragraph
        ]))
    }

    private static func appendCode(
        id: String,
        language: String?,
        content: String,
        presentation: AgentMarkdownPresentation,
        to output: NSMutableAttributedString,
        codeFont: NSFont,
        rhythm: MarkdownRhythm,
        codeHighlights: inout [MarkdownCodeHighlightRequest],
        codeBlocks: inout [MarkdownCodeBlockRegion]
    ) {
        // A transcript answer can carry a large tool dump. Laying the whole dump
        // out on the main thread costs a measurement pass per response, so the
        // card shows a capped body while the copy control still yields it all.
        // File Preview must show the whole file, so document mode keeps it.
        let displayed = presentation == .document
            ? content
            : AgentCodeBlockPolicy.displayedContent(content)
        // Marked here, not at the call site: appendFrontMatter alone has three
        // call sites, and a missed one takes the prose inset and collapses the
        // cell. A defer covers every return path.
        let bleedStart = output.length
        defer { markBleed(from: bleedStart, in: output) }
        let table = NSTextTable()
        table.numberOfColumns = 1
        table.collapsesBorders = true
        table.hidesEmptyCells = false
        table.setContentWidth(100, type: .percentageValueType)

        let headerBlock = NSTextTableBlock(
            table: table,
            startingRow: 0,
            rowSpan: 1,
            startingColumn: 0,
            columnSpan: 1
        )
        configureCodeBlock(
            headerBlock,
            backgroundColor: AppKitThemeAdapter.raised,
            horizontalPadding: AtelierMetrics.spaceM,
            verticalPadding: AtelierMetrics.spaceXS
        )
        let headerStyle = paragraphStyle(
            lineSpacing: 0,
            before: rhythm.codeCard,
            after: 0,
            firstLineHeadIndent: 0,
            headIndent: 0,
            tailIndent: -MarkdownCodeCardLayout.copyControlReservation
        )
        headerStyle.textBlocks = [headerBlock]
        let headerStart = output.length
        output.append(NSAttributedString(
            string: (language?.uppercased() ?? "CODE") + "\n",
            attributes: [
                .font: AtelierTypography.codeFont(
                    size: codeFont.pointSize
                        * AtelierTypography.micro
                        / AtelierTypography.uiSize
                ),
                .foregroundColor: AppKitThemeAdapter.secondary,
                .kern: 1.5,
                .paragraphStyle: headerStyle
            ]
        ))

        let bodyBlock = NSTextTableBlock(
            table: table,
            startingRow: 1,
            rowSpan: 1,
            startingColumn: 0,
            columnSpan: 1
        )
        configureCodeBlock(
            bodyBlock,
            // `code` equals the editor surface; a light raised wash sets the card apart.
            backgroundColor: AppKitThemeAdapter.raised.withAlphaComponent(
                MarkdownCodeCardLayout.bodyTintAlpha
            ),
            horizontalPadding: AtelierMetrics.spaceM,
            verticalPadding: AtelierMetrics.spaceS
        )
        let codeStyle = paragraphStyle(
            lineSpacing: rhythm.type.lineSpacing(
                for: codeFont,
                ratio: MarkdownTypeScale.code
            ),
            before: 0,
            after: 0,
            firstLineHeadIndent: AtelierMetrics.space2XL,
            headIndent: AtelierMetrics.space2XL,
            tailIndent: 0
        )
        codeStyle.tabStops = [
            NSTextTab(
                textAlignment: .right,
                location: AtelierMetrics.spaceXL,
                options: [:]
            ),
            NSTextTab(
                textAlignment: .left,
                location: AtelierMetrics.space2XL,
                options: [:]
            )
        ]
        codeStyle.defaultTabInterval = AtelierMetrics.space2XL
        codeStyle.textBlocks = [bodyBlock]
        let trailingCodeStyle = paragraphStyle(
            lineSpacing: rhythm.type.lineSpacing(
                for: codeFont,
                ratio: MarkdownTypeScale.code
            ),
            before: 0,
            after: rhythm.codeCard,
            firstLineHeadIndent: AtelierMetrics.space2XL,
            headIndent: AtelierMetrics.space2XL,
            tailIndent: 0
        )
        trailingCodeStyle.tabStops = codeStyle.tabStops
        trailingCodeStyle.defaultTabInterval = AtelierMetrics.space2XL
        trailingCodeStyle.textBlocks = [bodyBlock]
        let source = displayed.isEmpty ? " " : displayed
        let range = NSRange(location: output.length, length: (source as NSString).length)
        let code = NSMutableAttributedString(string: source + "\n", attributes: [
            .font: codeFont,
            .foregroundColor: AppKitThemeAdapter.foreground,
            .paragraphStyle: codeStyle
        ])
        let lastBreak = (source as NSString).range(of: "\n", options: .backwards)
        let lastLineStart = lastBreak.location == NSNotFound ? 0 : NSMaxRange(lastBreak)
        code.addAttribute(
            .paragraphStyle,
            value: trailingCodeStyle,
            range: NSRange(
                location: lastLineStart,
                length: code.length - lastLineStart
            )
        )
        var lineStart = 0
        for (lineIndex, line) in source.split(
            separator: "\n",
            omittingEmptySubsequences: false
        ).enumerated() where lineStart < source.utf16.count {
            code.addAttribute(
                .atelierCodeLineNumber,
                value: MarkdownCodeLineNumberDecoration(
                    number: lineIndex + 1,
                    codeFont: codeFont
                ),
                range: NSRange(location: lineStart, length: 1)
            )
            lineStart += line.utf16.count + 1
        }
        output.append(code)
        codeBlocks.append(
            MarkdownCodeBlockRegion(
                id: id,
                headerRange: NSRange(
                    location: headerStart,
                    length: range.location - headerStart
                ),
                sourceRange: range,
                source: AgentCodeBlockPolicy.copiedContent(content),
                usesGeneratedLineNumbers: true
            )
        )
        if !displayed.isEmpty {
            codeHighlights.append(
                MarkdownCodeHighlightRequest(
                    range: range,
                    source: displayed,
                    languageName: AgentCodeHighlightPolicy.languageName(for: language)
                )
            )
        }
    }

    private static func configureCodeBlock(
        _ block: NSTextTableBlock,
        backgroundColor: NSColor,
        horizontalPadding: CGFloat,
        verticalPadding: CGFloat
    ) {
        block.setWidth(
            AtelierTheme.strokeHairline,
            type: .absoluteValueType,
            for: .border
        )
        block.setBorderColor(AppKitThemeAdapter.border)
        block.setWidth(
            horizontalPadding,
            type: .absoluteValueType,
            for: .padding,
            edge: .minX
        )
        block.setWidth(
            horizontalPadding,
            type: .absoluteValueType,
            for: .padding,
            edge: .maxX
        )
        block.setWidth(
            verticalPadding,
            type: .absoluteValueType,
            for: .padding,
            edge: .minY
        )
        block.setWidth(
            verticalPadding,
            type: .absoluteValueType,
            for: .padding,
            edge: .maxY
        )
        block.backgroundColor = backgroundColor
    }

    /// Leading YAML metadata as one quiet two-column card in the same text storage.
    private static func appendFrontMatter(
        _ entries: [MarkdownFrontMatterEntry],
        to output: NSMutableAttributedString,
        bodySize: CGFloat,
        codeFont: NSFont,
        rhythm: MarkdownRhythm,
        measure: CGFloat
    ) {
        guard !entries.isEmpty else { return }
        // Marked here, not at the call site: appendFrontMatter alone has three
        // call sites, and a missed one takes the prose inset and collapses the
        // cell. A defer covers every return path.
        let bleedStart = output.length
        defer { markBleed(from: bleedStart, in: output) }
        let table = NSTextTable()
        table.numberOfColumns = 2
        table.collapsesBorders = true
        table.hidesEmptyCells = false
        table.setContentWidth(100, type: .percentageValueType)
        let keyFont = AtelierTypography.codeFont(size: max(9, bodySize * MarkdownTypeTokens.FontScale.frontMatterKey))
        let valueFont = NSFont.systemFont(ofSize: max(10, bodySize * MarkdownTypeTokens.FontScale.frontMatterValue))
        let background = AppKitThemeAdapter.raised.withAlphaComponent(0.26)
        // Size the key column from the widest key this document actually holds.
        // A fixed share fits the median key and wraps the deep dotted paths.
        let longestKey = entries.map(\.key).max { $0.count < $1.count } ?? ""
        let keyPercentage = MarkdownFrontMatterLayout.keyColumnPercentage(
            longestKeyWidth: (longestKey as NSString)
                .size(withAttributes: [.font: keyFont])
                .width,
            horizontalPadding: AtelierMetrics.spaceM + AtelierMetrics.spaceS,
            measure: measure
        )

        for (rowIndex, entry) in entries.enumerated() {
            for columnIndex in 0..<2 {
                let block = NSTextTableBlock(
                    table: table,
                    startingRow: rowIndex,
                    rowSpan: 1,
                    startingColumn: columnIndex,
                    columnSpan: 1
                )
                block.backgroundColor = background
                block.setBorderColor(AppKitThemeAdapter.border)
                if rowIndex == 0 {
                    block.setWidth(
                        AtelierTheme.strokeHairline,
                        type: .absoluteValueType,
                        for: .border,
                        edge: .minY
                    )
                }
                if rowIndex == entries.count - 1 {
                    block.setWidth(
                        AtelierTheme.strokeHairline,
                        type: .absoluteValueType,
                        for: .border,
                        edge: .maxY
                    )
                }
                block.setContentWidth(
                    columnIndex == 0 ? keyPercentage : 100 - keyPercentage,
                    type: .percentageValueType
                )
                block.setWidth(
                    AtelierMetrics.spaceM,
                    type: .absoluteValueType,
                    for: .padding,
                    edge: columnIndex == 0 ? .minX : .maxX
                )
                block.setWidth(
                    AtelierMetrics.spaceS,
                    type: .absoluteValueType,
                    for: .padding,
                    edge: columnIndex == 0 ? .maxX : .minX
                )
                block.setWidth(
                    AtelierMetrics.spaceXS,
                    type: .absoluteValueType,
                    for: .padding,
                    edge: .minY
                )
                block.setWidth(
                    AtelierMetrics.spaceXS,
                    type: .absoluteValueType,
                    for: .padding,
                    edge: .maxY
                )

                // Table cells keep zero paragraph spacing; the next block owns the gap.
                let paragraph = paragraphStyle(
                    lineSpacing: rhythm.type.lineSpacing(
                        for: valueFont,
                        ratio: MarkdownTypeScale.tableCell
                    ),
                    before: rowIndex == 0 ? rhythm.structure : 0,
                    after: 0
                )
                if columnIndex == 0 {
                    paragraph.lineBreakMode = .byCharWrapping
                }
                paragraph.textBlocks = [block]
                output.append(NSAttributedString(
                    string: (columnIndex == 0 ? entry.key : entry.value) + "\n",
                    attributes: [
                        .font: columnIndex == 0 ? keyFont : valueFont,
                        .foregroundColor: columnIndex == 0
                            ? AppKitThemeAdapter.accent
                            : AppKitThemeAdapter.secondary,
                        .kern: columnIndex == 0 ? 0.4 : 0,
                        .paragraphStyle: paragraph
                    ]
                ))
            }
        }
    }

    private static func appendMasthead(
        _ entries: [MarkdownFrontMatterEntry],
        to output: NSMutableAttributedString,
        bodySize: CGFloat,
        rhythm: MarkdownRhythm
    ) {
        guard !entries.isEmpty else { return }
        // Marked here, not at the call site: appendFrontMatter alone has three
        // call sites, and a missed one takes the prose inset and collapses the
        // cell. A defer covers every return path.
        let bleedStart = output.length
        defer { markBleed(from: bleedStart, in: output) }
        let table = NSTextTable()
        table.numberOfColumns = entries.count
        table.collapsesBorders = true
        table.hidesEmptyCells = false
        table.setContentWidth(100, type: .percentageValueType)
        let font = AtelierTypography.codeFont(size: max(9, bodySize * MarkdownTypeTokens.FontScale.calloutLabel))

        for (column, entry) in entries.enumerated() {
            let block = NSTextTableBlock(
                table: table,
                startingRow: 0,
                rowSpan: 1,
                startingColumn: column,
                columnSpan: 1
            )
            if column < entries.count - 1 {
                block.setBorderColor(AppKitThemeAdapter.border)
                block.setWidth(
                    AtelierTheme.strokeHairline,
                    type: .absoluteValueType,
                    for: .border,
                    edge: .maxX
                )
            }
            block.setWidth(
                AtelierMetrics.spaceS,
                type: .absoluteValueType,
                for: .padding,
                edge: .minX
            )
            block.setWidth(
                AtelierMetrics.spaceS,
                type: .absoluteValueType,
                for: .padding,
                edge: .maxX
            )
            let paragraph = paragraphStyle(
                lineSpacing: 0,
                before: rhythm.paragraph,
                after: 0
            )
            paragraph.alignment = .center
            paragraph.textBlocks = [block]
            output.append(NSAttributedString(
                string: entry.value + "\n",
                attributes: [
                    .font: font,
                    .foregroundColor: AppKitThemeAdapter.accent,
                    .kern: 0.6,
                    .paragraphStyle: paragraph
                ]
            ))
        }
    }

    private static func appendCallout(
        kind: MarkdownCalloutKind,
        content: String,
        to output: NSMutableAttributedString,
        bodyFont: NSFont,
        codeFont: NSFont,
        rhythm: MarkdownRhythm,
        footnoteNumbers: [String: Int]
    ) {
        // Marked here, not at the call site: appendFrontMatter alone has three
        // call sites, and a missed one takes the prose inset and collapses the
        // cell. A defer covers every return path.
        let bleedStart = output.length
        defer { markBleed(from: bleedStart, in: output) }
        let table = NSTextTable()
        table.numberOfColumns = 1
        table.collapsesBorders = true
        table.hidesEmptyCells = false
        table.setContentWidth(100, type: .percentageValueType)
        let block = NSTextTableBlock(
            table: table,
            startingRow: 0,
            rowSpan: 2,
            startingColumn: 0,
            columnSpan: 1
        )
        block.backgroundColor = kind.color.withAlphaComponent(0.07)
        block.setBorderColor(AppKitThemeAdapter.border)
        block.setWidth(
            AtelierTheme.strokeHairline,
            type: .absoluteValueType,
            for: .border
        )
        block.setBorderColor(kind.color, for: .minX)
        block.setWidth(
            MarkdownQuoteLayout.barWidth,
            type: .absoluteValueType,
            for: .border,
            edge: .minX
        )
        for edge in [NSRectEdge.minX, .maxX] {
            block.setWidth(
                AtelierMetrics.spaceM,
                type: .absoluteValueType,
                for: .padding,
                edge: edge
            )
        }
        for edge in [NSRectEdge.minY, .maxY] {
            block.setWidth(
                AtelierMetrics.spaceS,
                type: .absoluteValueType,
                for: .padding,
                edge: edge
            )
        }
        let labelStyle = paragraphStyle(
            lineSpacing: 0,
            before: rhythm.structure,
            after: AtelierMetrics.spaceXS
        )
        labelStyle.textBlocks = [block]
        output.append(NSAttributedString(
            string: "\(kind.glyph) \(kind.rawValue)\n",
            attributes: [
                .font: AtelierTypography.codeFont(
                    size: max(9, bodyFont.pointSize * MarkdownTypeTokens.FontScale.calloutLabel)
                ),
                .foregroundColor: kind.color,
                .kern: 0.7,
                .paragraphStyle: labelStyle
            ]
        ))
        let bodyStyle = paragraphStyle(
            lineSpacing: rhythm.type.lineSpacing(
                for: bodyFont,
                ratio: MarkdownTypeScale.prose
            ),
            before: 0,
            after: rhythm.structure
        )
        bodyStyle.textBlocks = [block]
        output.append(
            inlineText(
                content,
                font: bodyFont,
                foregroundColor: AppKitThemeAdapter.foreground,
                paragraphStyle: bodyStyle,
                codeFont: codeFont,
                footnoteNumbers: footnoteNumbers
            )
        )
        output.append(NSAttributedString(string: "\n", attributes: [
            .font: bodyFont,
            .paragraphStyle: bodyStyle
        ]))
    }

    private static func appendImageFigure(
        id: String,
        altText: String,
        urlText: String,
        sourceDirectoryURL: URL?,
        to output: NSMutableAttributedString,
        bodyFont: NSFont,
        rhythm: MarkdownRhythm,
        measure: CGFloat,
        imageFigures: inout [MarkdownImageFigureRegion]
    ) {
        // Reserve a stable placeholder box; the decoded image adopts its own aspect.
        let bounds = MarkdownImageFigureLayout.reservedBounds(measure: measure)
        let attachment = NSTextAttachment()
        attachment.bounds = bounds
        attachment.image = MarkdownImageFigureRenderer.image(
            size: bounds.size,
            content: nil
        )
        let paragraph = paragraphStyle(
            lineSpacing: 0,
            before: rhythm.structure,
            after: altText.isEmpty ? rhythm.structure : AtelierMetrics.spaceXS
        )
        paragraph.alignment = .center
        let figureLocation = output.length
        let figure = NSMutableAttributedString(attachment: attachment)
        figure.addAttribute(
            .paragraphStyle,
            value: paragraph,
            range: NSRange(location: 0, length: figure.length)
        )
        output.append(figure)
        output.append(NSAttributedString(string: "\n", attributes: [
            .font: bodyFont,
            .paragraphStyle: paragraph
        ]))

        if !altText.isEmpty {
            let captionFont = serifItalicFont(
                size: max(10, bodyFont.pointSize * MarkdownTypeTokens.FontScale.caption)
            )
            let caption = paragraphStyle(
                lineSpacing: rhythm.type.lineSpacing(
                    for: captionFont,
                    ratio: MarkdownTypeScale.tableCell
                ),
                before: 0,
                after: rhythm.structure
            )
            caption.alignment = .center
            output.append(NSAttributedString(
                string: altText + "\n",
                attributes: [
                    .font: captionFont,
                    .foregroundColor: AppKitThemeAdapter.secondary,
                    .paragraphStyle: caption
                ]
            ))
        }

        guard let url = MarkdownImageFigurePolicy.localURL(
            urlText: urlText,
            directoryURL: sourceDirectoryURL
        ) else {
            return
        }
        imageFigures.append(
            MarkdownImageFigureRegion(
                id: id,
                url: url,
                attachment: attachment,
                range: NSRange(location: figureLocation, length: figure.length)
            )
        )
    }

    /// Reserves a diagram card in the shared text storage. The rendered Mermaid
    /// image replaces the placeholder in place, without rebuilding the document.
    private static func appendMermaidFigure(
        id: String,
        source: String,
        to output: NSMutableAttributedString,
        bodyFont: NSFont,
        rhythm: MarkdownRhythm,
        measure: CGFloat,
        mermaidFigures: inout [MarkdownMermaidFigureRegion]
    ) {
        let bounds = MarkdownMermaidFigureLayout.reservedBounds(measure: measure)
        let attachment = NSTextAttachment()
        attachment.bounds = bounds
        attachment.image = MarkdownImageFigureRenderer.image(
            size: bounds.size,
            content: nil,
            message: MarkdownMermaidFigureLayout.loadingMessage
        )
        // The source toggle is pinned to the trailing edge of this range, so the
        // paragraph reserves the same width the reserved bounds already drop.
        let paragraph = paragraphStyle(
            lineSpacing: 0,
            before: rhythm.structure,
            after: rhythm.structure,
            tailIndent: -MarkdownCodeCardLayout.copyControlReservation
        )
        paragraph.alignment = .center
        let figureLocation = output.length
        let figure = NSMutableAttributedString(attachment: attachment)
        figure.addAttribute(
            .paragraphStyle,
            value: paragraph,
            range: NSRange(location: 0, length: figure.length)
        )
        output.append(figure)
        output.append(NSAttributedString(string: "\n", attributes: [
            .font: bodyFont,
            .paragraphStyle: paragraph
        ]))
        mermaidFigures.append(
            MarkdownMermaidFigureRegion(
                id: id,
                source: source,
                attachment: attachment,
                range: NSRange(location: figureLocation, length: figure.length)
            )
        )
    }

    private static func appendFootnotes(
        _ notes: [MarkdownFootnote],
        to output: NSMutableAttributedString,
        bodyFont: NSFont,
        codeFont: NSFont,
        rhythm: MarkdownRhythm
    ) {
        guard !notes.isEmpty else { return }
        // Marked here, not at the call site: appendFrontMatter alone has three
        // call sites, and a missed one takes the prose inset and collapses the
        // cell. A defer covers every return path.
        let bleedStart = output.length
        defer { markBleed(from: bleedStart, in: output) }
        let table = NSTextTable()
        table.numberOfColumns = 1
        table.collapsesBorders = true
        table.hidesEmptyCells = false
        table.setContentWidth(100, type: .percentageValueType)

        for row in 0...notes.count {
            let block = NSTextTableBlock(
                table: table,
                startingRow: row,
                rowSpan: 1,
                startingColumn: 0,
                columnSpan: 1
            )
            if row == 0 {
                block.setBorderColor(AppKitThemeAdapter.border)
                block.setWidth(
                    AtelierTheme.strokeHairline,
                    type: .absoluteValueType,
                    for: .border,
                    edge: .minY
                )
            }
            let paragraph = paragraphStyle(
                lineSpacing: rhythm.type.lineSpacing(
                    for: bodyFont,
                    ratio: MarkdownTypeScale.prose
                ),
                before: row == 0 ? rhythm.structure : 0,
                after: row == notes.count ? rhythm.structure : AtelierMetrics.spaceXS,
                firstLineHeadIndent: row == 0 ? 0 : AtelierMetrics.spaceXL,
                headIndent: row == 0 ? 0 : AtelierMetrics.spaceXL
            )
            paragraph.textBlocks = [block]
            if row == 0 {
                output.append(NSAttributedString(
                    string: "NOTES\n",
                    attributes: [
                        .font: serifFont(
                            size: max(10, bodyFont.pointSize * MarkdownTypeTokens.FontScale.footnotesTitle),
                            weight: .semibold
                        ),
                        .foregroundColor: AppKitThemeAdapter.secondary,
                        .kern: 0.6,
                        .paragraphStyle: paragraph
                    ]
                ))
            } else {
                let note = notes[row - 1]
                output.append(NSAttributedString(
                    string: "\(note.number).\t",
                    attributes: [
                        .font: AtelierTypography.codeFont(
                            size: max(9, bodyFont.pointSize * MarkdownTypeTokens.FontScale.footnoteNumber)
                        ),
                        .foregroundColor: AppKitThemeAdapter.accent,
                        .paragraphStyle: paragraph
                    ]
                ))
                output.append(
                    inlineText(
                        note.text,
                        font: serifFont(
                            size: max(10, bodyFont.pointSize * MarkdownTypeTokens.FontScale.footnoteText),
                            weight: .regular
                        ),
                        foregroundColor: AppKitThemeAdapter.secondary,
                        paragraphStyle: paragraph,
                        codeFont: codeFont
                    )
                )
                output.append(NSAttributedString(string: "\n", attributes: [
                    .font: bodyFont,
                    .paragraphStyle: paragraph
                ]))
            }
        }
    }

    private static func appendMermaid(
        source: String,
        error: String?,
        to output: NSMutableAttributedString,
        bodyFont: NSFont,
        codeFont: NSFont,
        rhythm: MarkdownRhythm
    ) {
        let headerStyle = paragraphStyle(
            lineSpacing: 0,
            before: AtelierMetrics.spaceL,
            after: AtelierMetrics.spaceXS,
            firstLineHeadIndent: AtelierMetrics.spaceM,
            headIndent: AtelierMetrics.spaceM,
            tailIndent: -AtelierMetrics.spaceM
        )
        output.append(NSAttributedString(string: "MERMAID - SOURCE FALLBACK\n", attributes: [
            .font: NSFont.monospacedSystemFont(
                ofSize: AtelierTypography.micro,
                weight: .semibold
            ),
            .foregroundColor: AppKitThemeAdapter.accent,
            .backgroundColor: AppKitThemeAdapter.raised,
            .paragraphStyle: headerStyle
        ]))
        if let error {
            output.append(NSAttributedString(string: error + "\n", attributes: [
                .font: bodyFont,
                .foregroundColor: AppKitThemeAdapter.secondary,
                .backgroundColor: AppKitThemeAdapter.raised,
                .paragraphStyle: headerStyle
            ]))
        }
        let sourceStyle = paragraphStyle(
            lineSpacing: rhythm.type.lineSpacing(
                for: codeFont,
                ratio: MarkdownTypeScale.code
            ),
            before: 0,
            after: AtelierMetrics.spaceL,
            firstLineHeadIndent: AtelierMetrics.spaceM,
            headIndent: AtelierMetrics.spaceM,
            tailIndent: -AtelierMetrics.spaceM
        )
        output.append(NSAttributedString(string: source + "\n", attributes: [
            .font: codeFont,
            .foregroundColor: AppKitThemeAdapter.foreground,
            .backgroundColor: AppKitThemeAdapter.code,
            .paragraphStyle: sourceStyle
        ]))
    }

    private static func appendTable(
        headers: [String],
        alignments: [MarkdownColumnAlignment],
        rows: [[String]],
        to output: NSMutableAttributedString,
        bodyFont: NSFont,
        codeFont: NSFont,
        usesDarkAppearance: Bool,
        rhythm: MarkdownRhythm,
        footnoteNumbers: [String: Int]
    ) {
        guard !headers.isEmpty else { return }
        // Marked here, not at the call site: appendFrontMatter alone has three
        // call sites, and a missed one takes the prose inset and collapses the
        // cell. A defer covers every return path.
        let bleedStart = output.length
        defer { markBleed(from: bleedStart, in: output) }
        let table = NSTextTable()
        table.numberOfColumns = headers.count
        table.collapsesBorders = true
        table.hidesEmptyCells = false
        table.setContentWidth(100, type: .percentageValueType)
        let allRows = [headers] + rows
        let columnPercentages = tableColumnPercentages(headers: headers, rows: rows)
        let numericColumns = MarkdownTableAlignmentPolicy.numericMajorityColumns(
            headers: headers,
            rows: rows
        )
        let spacerStyle = paragraphStyle(
            lineSpacing: 0,
            before: rhythm.structure,
            after: 0
        )
        output.append(NSAttributedString(string: "\n", attributes: [
            .font: NSFont.systemFont(ofSize: 1),
            .paragraphStyle: spacerStyle
        ]))

        for (rowIndex, row) in allRows.enumerated() {
            for columnIndex in headers.indices {
                let block = NSTextTableBlock(
                    table: table,
                    startingRow: rowIndex,
                    rowSpan: 1,
                    startingColumn: columnIndex,
                    columnSpan: 1
                )
                block.setBorderColor(AppKitThemeAdapter.border)
                block.setWidth(
                    AtelierTheme.strokeHairline,
                    type: .absoluteValueType,
                    for: .border,
                    edge: .maxY
                )
                if rowIndex == 0 {
                    block.setWidth(
                        AtelierTheme.strokeHairline,
                        type: .absoluteValueType,
                        for: .border,
                        edge: .minY
                    )
                }
                block.setContentWidth(
                    columnPercentages[columnIndex],
                    type: .percentageValueType
                )
                block.setWidth(
                    AtelierMetrics.spaceM,
                    type: .absoluteValueType,
                    for: .padding,
                    edge: .minX
                )
                block.setWidth(
                    AtelierMetrics.spaceM,
                    type: .absoluteValueType,
                    for: .padding,
                    edge: .maxX
                )
                block.setWidth(
                    AtelierMetrics.spaceS,
                    type: .absoluteValueType,
                    for: .padding,
                    edge: .minY
                )
                block.setWidth(
                    AtelierMetrics.spaceS,
                    type: .absoluteValueType,
                    for: .padding,
                    edge: .maxY
                )
                if rowIndex == 0 {
                    // Same family as the zebra rows, several steps stronger, so the
                    // header stays distinct without spending the accent on it.
                    block.backgroundColor = AppKitThemeAdapter.raised.withAlphaComponent(
                        usesDarkAppearance ? 0.55 : 0.60
                    )
                } else if rowIndex.isMultiple(of: 2) {
                    block.backgroundColor = AppKitThemeAdapter.raised.withAlphaComponent(
                        usesDarkAppearance ? 0.26 : 0.30
                    )
                }

                let value = row.indices.contains(columnIndex) ? row[columnIndex] : ""
                let cellSize = bodyFont.pointSize * MarkdownTableCellPolicy.fontScale
                let weight: NSFont.Weight = rowIndex == 0 ? .semibold : .regular
                let font = numericColumns.contains(columnIndex)
                    ? NSFont.monospacedDigitSystemFont(
                        ofSize: cellSize,
                        weight: weight
                    )
                    : NSFont.systemFont(ofSize: cellSize, weight: weight)
                let paragraph = paragraphStyle(
                    lineSpacing: rhythm.type.lineSpacing(
                        for: font,
                        ratio: MarkdownTypeScale.tableCell
                    ),
                    before: 0,
                    after: 0
                )
                // Long unbroken tokens (paths, URLs) must wrap inside their column.
                if MarkdownTableCellPolicy.wrapsByCharacter(value) {
                    paragraph.lineBreakMode = .byCharWrapping
                }
                let declaredAlignment = alignments.indices.contains(columnIndex)
                    ? alignments[columnIndex]
                    : .left
                paragraph.alignment = nativeAlignment(
                    numericColumns.contains(columnIndex)
                        ? .right
                        : declaredAlignment
                )
                paragraph.textBlocks = [block]
                let cell = inlineText(
                    value,
                    font: font,
                    foregroundColor: AppKitThemeAdapter.foreground,
                    paragraphStyle: paragraph,
                    codeFont: codeFont,
                    footnoteNumbers: footnoteNumbers
                )
                if rowIndex == 0 {
                    cell.addAttribute(
                        .kern,
                        value: 0.4,
                        range: NSRange(location: 0, length: cell.length)
                    )
                }
                output.append(cell)
                output.append(NSAttributedString(string: "\n", attributes: [
                    .font: font,
                    .paragraphStyle: paragraph
                ]))
            }
        }

        // Cells carry no trailing gap, so the table needs its own closing edge at
        // the structure tier instead of inheriting the next block's flow gap.
        let trailingStyle = paragraphStyle(
            lineSpacing: 0,
            before: 0,
            after: rhythm.structure
        )
        output.append(NSAttributedString(string: "\n", attributes: [
            .font: NSFont.systemFont(ofSize: 1),
            .paragraphStyle: trailingStyle
        ]))
    }

    static func tableColumnPercentages(
        headers: [String],
        rows: [[String]]
    ) -> [CGFloat] {
        guard !headers.isEmpty else { return [] }
        let sample = rows.prefix(128)
        let scores = headers.indices.map { columnIndex -> CGFloat in
            let values = [headers[columnIndex]] + sample.map { row in
                row.indices.contains(columnIndex) ? row[columnIndex] : ""
            }
            let longest = values.reduce(1) { current, value in
                let lineLength = value.split(
                    separator: "\n",
                    omittingEmptySubsequences: false
                ).reduce(0) { max($0, min(80, $1.count)) }
                return max(current, lineLength)
            }
            return sqrt(CGFloat(longest) + 4)
        }
        let scoreTotal = scores.reduce(0, +)
        let equalShare = 100 / CGFloat(headers.count)
        let minimumShare = min(14, equalShare * 0.72)
        let maximumShare = max(equalShare, min(48, equalShare * 2.4))
        let blended = scores.map { score in
            let weightedShare = scoreTotal > 0 ? score / scoreTotal * 100 : equalShare
            return min(
                maximumShare,
                max(minimumShare, equalShare * 0.3 + weightedShare * 0.7)
            )
        }
        let blendedTotal = blended.reduce(0, +)
        guard blendedTotal > 0 else {
            return Array(repeating: equalShare, count: headers.count)
        }
        return blended.map { $0 / blendedTotal * 100 }
    }

    private static func inlineText(
        _ content: String,
        font: NSFont,
        foregroundColor: NSColor,
        paragraphStyle: NSParagraphStyle,
        codeFont: NSFont,
        footnoteNumbers: [String: Int] = [:]
    ) -> NSMutableAttributedString {
        let swiftValue = AgentMarkdownInlinePolicy.attributedString(
            content,
            showsColorSwatches: true,
            footnoteNumbers: footnoteNumbers
        )
        let output = NSMutableAttributedString(attributedString: NSAttributedString(swiftValue))
        let fullRange = NSRange(location: 0, length: output.length)
        guard fullRange.length > 0 else { return output }

        if !footnoteNumbers.isEmpty {
            let source = content as NSString
            for reference in MarkdownFootnotePolicy.resolvedReferences(
                in: content,
                numbers: footnoteNumbers
            ) {
                let prefix = source.substring(to: reference.sourceRange.location)
                let renderedPrefix = AgentMarkdownInlinePolicy.attributedString(
                    prefix,
                    showsColorSwatches: true,
                    footnoteNumbers: footnoteNumbers
                )
                let range = NSRange(
                    location: NSAttributedString(renderedPrefix).length,
                    length: String(reference.number).utf16.count
                )
                guard NSMaxRange(range) <= output.length else { continue }
                output.addAttributes([
                    .font: NSFont.systemFont(
                        ofSize: AtelierTypography.micro,
                        weight: .semibold
                    ),
                    .foregroundColor: AppKitThemeAdapter.accent,
                    .baselineOffset: 4
                ], range: range)
            }
        }
        let nativeString = output.string as NSString
        var swatchColors: [(range: NSRange, color: NSColor)] = []
        let matches = colorSwatchExpression?.matches(
            in: output.string,
            range: fullRange
        ) ?? []
        for match in matches {
            let tokenRange = match.range(at: 1)
            guard tokenRange.location != NSNotFound,
                  let color = nativeColor(
                    for: nativeString.substring(with: tokenRange)
                  ) else {
                continue
            }
            swatchColors.append(
                (
                    NSRange(location: match.range.location, length: 1),
                    color
                )
            )
        }

        fillMissingAttribute(.font, value: font, in: output, range: fullRange)
        fillMissingAttribute(
            .foregroundColor,
            value: foregroundColor,
            in: output,
            range: fullRange
        )
        output.addAttribute(.paragraphStyle, value: paragraphStyle, range: fullRange)

        var intentRuns: [(Int, NSRange)] = []
        output.enumerateAttribute(.inlinePresentationIntent, in: fullRange) { value, range, _ in
            let rawValue = (value as? NSNumber)?.intValue ?? (value as? Int ?? 0)
            if rawValue != 0 {
                intentRuns.append((rawValue, range))
            }
        }
        for (intent, range) in intentRuns {
            if intent & 4 != 0 {
                // The mono face alone sets code apart, the way a printed book does.
                // Any fill turns a code-heavy paragraph into a mosaic of blocks, and
                // with no fill there is nothing to clear, so no kern is reserved and
                // a following full stop sits tight against the run.
                output.removeAttribute(.backgroundColor, range: range)
                output.addAttributes([
                    .font: AtelierTypography.codeFont(
                        size: font.pointSize * MarkdownInlineCodePolicy.fontScale,
                        ligatures: false
                    ),
                    .foregroundColor: AppKitThemeAdapter.foreground
                ], range: range)
                continue
            }
            var traits: NSFontDescriptor.SymbolicTraits = []
            if intent & 1 != 0 { traits.insert(.italic) }
            if intent & 2 != 0 { traits.insert(.bold) }
            if !traits.isEmpty {
                let descriptor = font.fontDescriptor.withSymbolicTraits(traits)
                if let styledFont = NSFont(
                    descriptor: descriptor,
                    size: font.pointSize
                ) {
                    output.addAttribute(.font, value: styledFont, range: range)
                }
            }
        }
        for swatch in swatchColors {
            output.addAttribute(
                .foregroundColor,
                value: swatch.color,
                range: swatch.range
            )
        }
        return output
    }

    private static func nativeColor(for token: String) -> NSColor? {
        guard token.first == "#" else { return nil }
        let hex = token.dropFirst()
        let expanded: String
        switch hex.count {
        case 3:
            expanded = hex.reduce(into: "") { result, digit in
                result.append(digit)
                result.append(digit)
            } + "FF"
        case 4:
            expanded = hex.reduce(into: "") { result, digit in
                result.append(digit)
                result.append(digit)
            }
        case 6:
            expanded = String(hex) + "FF"
        case 8:
            expanded = String(hex)
        default:
            return nil
        }
        guard let rgba = UInt32(expanded, radix: 16) else { return nil }
        return NSColor(
            srgbRed: CGFloat((rgba >> 24) & 0xFF) / 255,
            green: CGFloat((rgba >> 16) & 0xFF) / 255,
            blue: CGFloat((rgba >> 8) & 0xFF) / 255,
            alpha: CGFloat(rgba & 0xFF) / 255
        )
    }

    private static func fillMissingAttribute(
        _ key: NSAttributedString.Key,
        value: Any,
        in output: NSMutableAttributedString,
        range: NSRange
    ) {
        var missing: [NSRange] = []
        output.enumerateAttribute(key, in: range) { current, currentRange, _ in
            if current == nil {
                missing.append(currentRange)
            }
        }
        for range in missing {
            output.addAttribute(key, value: value, range: range)
        }
    }

    private static func markBleed(from start: Int, in output: NSMutableAttributedString) {
        guard output.length > start else { return }
        output.addAttribute(
            .atelierBleedBlock,
            value: true,
            range: NSRange(location: start, length: output.length - start)
        )
    }

    /// Hold prose on its own measure inside a wider container. Runs once per build,
    /// after every block is emitted, so the block appenders stay unaware of it.
    private static func applyProseInset(
        _ inset: CGFloat,
        to output: NSMutableAttributedString
    ) {
        guard inset > 0, output.length > 0 else { return }
        let full = NSRange(location: 0, length: output.length)
        output.enumerateAttribute(.atelierBleedBlock, in: full) { bleed, range, _ in
            guard bleed == nil else { return }
            output.enumerateAttribute(.paragraphStyle, in: range) { value, styleRange, _ in
                guard let style = value as? NSParagraphStyle,
                      let inset_style = style.mutableCopy() as? NSMutableParagraphStyle else {
                    return
                }
                inset_style.firstLineHeadIndent += inset
                inset_style.headIndent += inset
                // tailIndent is measured from the trailing edge when negative, and
                // from the leading edge when positive. Only the negative form needs
                // to move; zero means "the container edge", which becomes the inset.
                inset_style.tailIndent = inset_style.tailIndent < 0
                    ? inset_style.tailIndent - inset
                    : -inset
                output.addAttribute(
                    .paragraphStyle,
                    value: inset_style,
                    range: styleRange
                )
            }
        }
    }

    private static func paragraphStyle(
        lineSpacing: CGFloat,
        before: CGFloat,
        after: CGFloat,
        firstLineHeadIndent: CGFloat = 0,
        headIndent: CGFloat = 0,
        tailIndent: CGFloat = 0
    ) -> NSMutableParagraphStyle {
        let style = NSMutableParagraphStyle()
        style.lineBreakMode = .byWordWrapping
        style.lineSpacing = lineSpacing
        style.paragraphSpacingBefore = before
        style.paragraphSpacing = after
        style.firstLineHeadIndent = firstLineHeadIndent
        style.headIndent = headIndent
        style.tailIndent = tailIndent
        return style
    }

    /// Pure ratios, never a minimum clamp. A floor changes the hierarchy's shape
    /// as the reader resizes text: an H1 clamped to 28 points reads 2.6 times body
    /// at the small end and 1.85 times body at the large end.
    private static func headingRatio(
        level: Int,
        presentation: AgentMarkdownPresentation
    ) -> CGFloat {
        MarkdownTypeTokens.headingRatio(
            level: level,
            isDocument: presentation == .document
        )
    }

    private static func headingFont(
        level: Int,
        bodySize: CGFloat,
        presentation: AgentMarkdownPresentation
    ) -> NSFont {
        let size = bodySize * headingRatio(level: level, presentation: presentation)
        return level <= 2
            ? serifFont(size: size, weight: .semibold)
            : NSFont.systemFont(ofSize: size, weight: .semibold)
    }

    private static func serifFont(size: CGFloat, weight: NSFont.Weight) -> NSFont {
        let base = NSFont.systemFont(ofSize: size, weight: weight)
        guard let descriptor = base.fontDescriptor.withDesign(.serif) else { return base }
        return NSFont(descriptor: descriptor, size: size) ?? base
    }

    /// The face body prose renders with. Document mode reads as an editorial page,
    /// so its prose shares the serif face of H1/H2; transcript prose stays system.
    /// The rhythm unit derives from this font, so tests build it from here too.
    static func documentBodyFont(
        size: CGFloat,
        presentation: AgentMarkdownPresentation
    ) -> NSFont {
        presentation == .document
            ? serifFont(size: size, weight: .regular)
            : NSFont.systemFont(ofSize: size)
    }

    private static func serifItalicFont(size: CGFloat) -> NSFont {
        let base = NSFont.systemFont(ofSize: size)
        guard let serif = base.fontDescriptor.withDesign(.serif) else { return base }
        let italic = serif.withSymbolicTraits(.italic)
        return NSFont(descriptor: italic, size: size)
            ?? NSFont(descriptor: serif, size: size)
            ?? base
    }

    private static func headingColor(
        level: Int,
        presentation: AgentMarkdownPresentation
    ) -> NSColor {
        switch level {
        case 1, 2, 4:
            // H4 sits at body size, so secondary ink would make the heading read
            // quieter than the text under it.
            AppKitThemeAdapter.foreground
        case 3:
            // The accent eyebrow is a document-only treatment.
            presentation == .document
                ? AppKitThemeAdapter.accent
                : AppKitThemeAdapter.foreground
        default:
            AppKitThemeAdapter.secondary
        }
    }

    private static func headingTopSpacing(
        level: Int,
        rhythm: MarkdownRhythm
    ) -> CGFloat {
        level <= 2 ? rhythm.headingPrimaryBefore : rhythm.heading3Before
    }

    private static func unorderedMarker(depth: Int) -> String {
        switch depth {
        case 0: "\u{2022}"
        case 1: "\u{25E6}"
        default: "\u{2013}"
        }
    }

    private static func nativeAlignment(
        _ alignment: MarkdownColumnAlignment
    ) -> NSTextAlignment {
        switch alignment {
        case .left: .left
        case .center: .center
        case .right: .right
        }
    }

    private static func blendedColor(
        from: NSColor,
        to: NSColor,
        fraction: CGFloat
    ) -> NSColor {
        from.blended(
            withFraction: min(1, max(0, fraction)),
            of: to
        ) ?? from
    }
}

struct MarkdownCodeCopyControl: View {
    let source: String

    /// `MarkdownCopyButton` uses `AtelierGhostButtonStyle`, which already ends in
    /// `.atelierPointerCursor()`. Do not apply the pointer cursor a second time.
    var body: some View {
        MarkdownCopyButton(source: source, label: "Copy code")
    }
}

/// Reveals and hides the Mermaid source under a rendered figure.
/// `AtelierGhostButtonStyle` already ends in `.atelierPointerCursor()`, so the
/// pointer cursor is not applied a second time here.
struct MarkdownMermaidSourceControl: View {
    let isExpanded: Bool
    let toggle: () -> Void

    private var label: String {
        isExpanded ? "Hide Mermaid source" : "View Mermaid source"
    }

    var body: some View {
        Button(action: toggle) {
            Image(
                systemName: isExpanded
                    ? "chevron.up"
                    : "chevron.left.forwardslash.chevron.right"
            )
            .frame(width: AtelierMetrics.regularIconSize)
        }
        .buttonStyle(
            AtelierGhostButtonStyle(
                tint: isExpanded ? AtelierTheme.accent : .primary
            )
        )
        .accessibilityLabel(label)
        .help(label)
    }
}

nonisolated enum MarkdownOverlayAnchor: Equatable, Sendable {
    /// Centered on the anchored range, for a code-card header row.
    case centered
    /// Pinned to the top of the anchored range, for a figure.
    case top
}

/// Trailing overlay geometry shared by the file-preview and transcript surfaces.
/// Controls are pinned to a TextKit range, never inserted into the document.
@MainActor
enum MarkdownOverlayControlLayout {
    static let inset = AtelierMetrics.spaceS

    static func visibleCharacterRange(
        in textView: NSTextView,
        visibleRect: NSRect
    ) -> NSRange? {
        guard let textContainer = textView.textContainer,
              let layoutManager = textView.layoutManager else { return nil }
        let origin = textView.textContainerOrigin
        let containerRect = visibleRect.offsetBy(dx: -origin.x, dy: -origin.y)
        let glyphRange = layoutManager.glyphRange(
            forBoundingRect: containerRect,
            in: textContainer
        )
        return layoutManager.characterRange(
            forGlyphRange: glyphRange,
            actualGlyphRange: nil
        )
    }

    static func frame(
        for range: NSRange,
        size: NSSize,
        anchor: MarkdownOverlayAnchor,
        in textView: NSTextView,
        host: NSView
    ) -> NSRect? {
        guard let textContainer = textView.textContainer,
              let layoutManager = textView.layoutManager,
              let textStorage = textView.textStorage,
              range.length > 0,
              NSMaxRange(range) <= textStorage.length else {
            return nil
        }
        let glyphRange = layoutManager.glyphRange(
            forCharacterRange: range,
            actualCharacterRange: nil
        )
        let rect = layoutManager.boundingRect(
            forGlyphRange: glyphRange,
            in: textContainer
        )
        let textOrigin = textView.textContainerOrigin
        let y = switch anchor {
        case .centered: textOrigin.y + rect.midY - size.height / 2
        case .top: textOrigin.y + rect.minY + inset
        }
        let origin = textView.convert(
            NSPoint(
                x: textView.bounds.maxX
                    - textView.textContainerInset.width
                    - inset
                    - size.width,
                y: y
            ),
            to: host
        )
        return NSRect(
            x: origin.x,
            y: origin.y,
            width: size.width,
            height: size.height
        ).integral
    }
}

/// Owns the revealed Mermaid source ranges inside one text storage. A toggle
/// edits the storage in place; it never rebuilds the attributed document.
@MainActor
final class MarkdownMermaidSourceExpansion {
    private var insertedRanges: [String: NSRange] = [:]

    func isExpanded(_ id: String) -> Bool {
        insertedRanges[id] != nil
    }

    func reset() {
        insertedRanges.removeAll(keepingCapacity: true)
    }

    /// Inserts or removes the source block. Returns the edit so the caller can
    /// shift the document regions it owns.
    @discardableResult
    func toggle(
        figure: MarkdownMermaidFigureRegion,
        in textStorage: NSTextStorage,
        scale: CGFloat,
        displayScale: CGFloat,
        presentation: AgentMarkdownPresentation
    ) -> (location: Int, delta: Int)? {
        if let existing = insertedRanges[figure.id] {
            guard NSMaxRange(existing) <= textStorage.length else {
                insertedRanges[figure.id] = nil
                return nil
            }
            textStorage.beginEditing()
            textStorage.deleteCharacters(in: existing)
            textStorage.endEditing()
            insertedRanges[figure.id] = nil
            shift(after: existing.location, by: -existing.length)
            return (existing.location, -existing.length)
        }
        guard NSMaxRange(figure.range) <= textStorage.length else { return nil }
        // Land after the figure's own paragraph break.
        let location = min(textStorage.length, NSMaxRange(figure.range) + 1)
        let block = MarkdownAttributedDocumentBuilder.mermaidSourceBlock(
            source: figure.source,
            scale: scale,
            displayScale: displayScale,
            presentation: presentation
        )
        guard block.length > 0 else { return nil }
        textStorage.beginEditing()
        textStorage.insert(block, at: location)
        textStorage.endEditing()
        // Shift the other expansions first: the new range starts at the edit
        // location and must not shift itself.
        shift(after: location, by: block.length)
        insertedRanges[figure.id] = NSRange(
            location: location,
            length: block.length
        )
        return (location, block.length)
    }

    private func shift(after location: Int, by delta: Int) {
        // An insert lands exactly where the next block starts, so a range at the
        // edit location moves too. Use the same rule as `MarkdownRegionShift`.
        for (id, range) in insertedRanges where range.location >= location {
            insertedRanges[id] = NSRange(
                location: range.location + delta,
                length: range.length
            )
        }
    }
}

/// Keeps stored TextKit ranges valid after an in-place storage edit.
nonisolated enum MarkdownRegionShift {
    /// A region that starts exactly at the edit location moves too: the Mermaid
    /// source lands on the first index of the block that follows the figure.
    static func shifted(
        _ range: NSRange,
        after location: Int,
        by delta: Int
    ) -> NSRange {
        guard range.location >= location else { return range }
        return NSRange(
            location: max(0, range.location + delta),
            length: range.length
        )
    }
}

nonisolated struct MarkdownDecodedImage: Sendable {
    let width: Int
    let height: Int
    let pixels: Data

    @MainActor
    func cgImage() -> CGImage? {
        guard width > 0,
              height > 0,
              pixels.count == width * height * 4,
              let colorSpace = CGColorSpace(name: CGColorSpace.sRGB),
              let provider = CGDataProvider(data: pixels as CFData) else {
            return nil
        }
        return CGImage(
            width: width,
            height: height,
            bitsPerComponent: 8,
            bitsPerPixel: 32,
            bytesPerRow: width * 4,
            space: colorSpace,
            bitmapInfo: CGBitmapInfo(
                rawValue: CGImageAlphaInfo.premultipliedLast.rawValue
            ),
            provider: provider,
            decode: nil,
            shouldInterpolate: true,
            intent: .defaultIntent
        )
    }
}

@MainActor
enum MarkdownImageFigureRenderer {
    static func image(
        size: NSSize,
        content: CGImage?,
        message: String? = nil
    ) -> NSImage {
        let width = max(1, Int(size.width.rounded(.up)))
        let height = max(1, Int(size.height.rounded(.up)))
        guard let colorSpace = CGColorSpace(name: CGColorSpace.sRGB),
              let context = CGContext(
                  data: nil,
                  width: width,
                  height: height,
                  bitsPerComponent: 8,
                  bytesPerRow: 0,
                  space: colorSpace,
                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
              ) else {
            return NSImage(size: size)
        }

        let canvas = CGRect(x: 0, y: 0, width: width, height: height)
        let borderWidth = max(1, AtelierTheme.strokeHairline)
        let frame = canvas.insetBy(dx: borderWidth / 2, dy: borderWidth / 2)
        let shape = CGPath(
            roundedRect: frame,
            cornerWidth: AtelierTheme.controlRadius,
            cornerHeight: AtelierTheme.controlRadius,
            transform: nil
        )
        context.setFillColor(
            AppKitThemeAdapter.raised.withAlphaComponent(0.45).cgColor
        )
        context.addPath(shape)
        context.fillPath()

        if let content {
            let sourceSize = CGSize(
                width: content.width,
                height: content.height
            )
            let fit = min(
                frame.width / sourceSize.width,
                frame.height / sourceSize.height
            )
            let fittedSize = CGSize(
                width: sourceSize.width * fit,
                height: sourceSize.height * fit
            )
            let fitted = CGRect(
                x: frame.midX - fittedSize.width / 2,
                y: frame.midY - fittedSize.height / 2,
                width: fittedSize.width,
                height: fittedSize.height
            )
            context.saveGState()
            context.addPath(shape)
            context.clip()
            context.interpolationQuality = .high
            context.draw(content, in: fitted)
            context.restoreGState()
        } else if let message {
            let label = NSAttributedString(
                string: message,
                attributes: [
                    .font: NSFont.systemFont(ofSize: AtelierTypography.caption),
                    .foregroundColor: AppKitThemeAdapter.secondary
                ]
            )
            let line = CTLineCreateWithAttributedString(label)
            let labelBounds = CTLineGetBoundsWithOptions(line, .useOpticalBounds)
            context.saveGState()
            context.textPosition = CGPoint(
                x: frame.midX - labelBounds.width / 2,
                y: frame.midY - labelBounds.height / 2
            )
            CTLineDraw(line, context)
            context.restoreGState()
        }

        context.setStrokeColor(AppKitThemeAdapter.border.cgColor)
        context.setLineWidth(borderWidth)
        context.addPath(shape)
        context.strokePath()
        guard let rendered = context.makeImage() else {
            return NSImage(size: size)
        }
        return NSImage(cgImage: rendered, size: size)
    }
}

nonisolated enum MarkdownLocalImageLoader {
    static func load(_ url: URL) async -> MarkdownDecodedImage? {
        guard !Task.isCancelled else { return nil }
        return await withTaskGroup(
            of: MarkdownDecodedImage?.self,
            returning: MarkdownDecodedImage?.self
        ) { group in
            group.addTask(priority: .utility) {
                decode(url)
            }
            return await group.next() ?? nil
        }
    }

    private static func decode(_ url: URL) -> MarkdownDecodedImage? {
        guard !Task.isCancelled,
              url.isFileURL,
              let source = CGImageSourceCreateWithURL(
                  url as CFURL,
                  [
                      kCGImageSourceShouldCache: false,
                      kCGImageSourceShouldCacheImmediately: true
                  ] as CFDictionary
              ),
              let image = CGImageSourceCreateImageAtIndex(
                  source,
                  0,
                  [
                      kCGImageSourceShouldCacheImmediately: true
                  ] as CFDictionary
              ),
              !Task.isCancelled else {
            return nil
        }
        let width = image.width
        let height = image.height
        let bytesPerRow = width * 4
        var pixels = Data(count: bytesPerRow * height)
        let didRender = pixels.withUnsafeMutableBytes { buffer in
            guard !Task.isCancelled,
                  let address = buffer.baseAddress,
                  let colorSpace = CGColorSpace(name: CGColorSpace.sRGB),
                  let context = CGContext(
                      data: address,
                      width: width,
                      height: height,
                      bitsPerComponent: 8,
                      bytesPerRow: bytesPerRow,
                      space: colorSpace,
                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
                  ) else {
                return false
            }
            context.draw(
                image,
                in: CGRect(x: 0, y: 0, width: width, height: height)
            )
            return true
        }
        guard didRender, !Task.isCancelled else { return nil }
        return MarkdownDecodedImage(
            width: width,
            height: height,
            pixels: pixels
        )
    }
}

@MainActor
final class MarkdownPreviewTextView: NSTextView {
    private var linkTrackingArea: NSTrackingArea?
    private var hoveredLinkRange: NSRange?
    var quietLinkUnderlineColor = MarkdownLinkStylePolicy.normalUnderlineColor
    var activeLinkUnderlineColor = AppKitThemeAdapter.accent

    override func updateTrackingAreas() {
        if let linkTrackingArea {
            removeTrackingArea(linkTrackingArea)
        }
        let area = NSTrackingArea(
            rect: .zero,
            options: [
                .activeInKeyWindow,
                .inVisibleRect,
                .mouseMoved,
                .mouseEnteredAndExited
            ],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        linkTrackingArea = area
        super.updateTrackingAreas()
    }

    override func mouseMoved(with event: NSEvent) {
        super.mouseMoved(with: event)
        let point = convert(event.locationInWindow, from: nil)
        let range = linkRange(at: point)
        updateHoveredLink(range)
        if range != nil {
            NSCursor.pointingHand.set()
        } else {
            NSCursor.iBeam.set()
        }
    }

    func linkRange(at point: NSPoint) -> NSRange? {
        guard let textContainer,
              let layoutManager,
              let textStorage else {
            return nil
        }
        let containerPoint = NSPoint(
            x: point.x - textContainerOrigin.x,
            y: point.y - textContainerOrigin.y
        )
        let glyphIndex = layoutManager.glyphIndex(
            for: containerPoint,
            in: textContainer,
            fractionOfDistanceThroughGlyph: nil
        )
        guard glyphIndex < layoutManager.numberOfGlyphs else {
            return nil
        }
        let characterIndex = layoutManager.characterIndexForGlyph(at: glyphIndex)
        guard characterIndex < textStorage.length else {
            return nil
        }
        var range = NSRange()
        let link = textStorage.attribute(
            .link,
            at: characterIndex,
            longestEffectiveRange: &range,
            in: NSRange(location: 0, length: textStorage.length)
        )
        guard link != nil else { return nil }
        let glyphRange = layoutManager.glyphRange(
            forCharacterRange: range,
            actualCharacterRange: nil
        )
        var containsPoint = false
        layoutManager.enumerateEnclosingRects(
            forGlyphRange: glyphRange,
            withinSelectedGlyphRange: NSRange(location: NSNotFound, length: 0),
            in: textContainer
        ) { rect, stop in
            guard rect.contains(containerPoint) else { return }
            containsPoint = true
            stop.pointee = true
        }
        return containsPoint ? range : nil
    }

    override func mouseExited(with event: NSEvent) {
        updateHoveredLink(nil)
        super.mouseExited(with: event)
    }

    func resetHoveredLink() {
        updateHoveredLink(nil)
    }

    private func updateHoveredLink(_ range: NSRange?) {
        guard hoveredLinkRange != range, let textStorage else { return }
        textStorage.beginEditing()
        if let hoveredLinkRange,
           NSMaxRange(hoveredLinkRange) <= textStorage.length {
            textStorage.addAttribute(
                .underlineColor,
                value: quietLinkUnderlineColor,
                range: hoveredLinkRange
            )
        }
        if let range, NSMaxRange(range) <= textStorage.length {
            textStorage.addAttribute(
                .underlineColor,
                value: activeLinkUnderlineColor,
                range: range
            )
        }
        textStorage.endEditing()
        hoveredLinkRange = range
    }
}

struct MarkdownSelectableDocumentView: NSViewRepresentable {
    @Environment(\.atelierZoomScale) private var scale
    @Environment(\.displayScale) private var displayScale
    @Environment(\.colorScheme) private var colorScheme

    let document: ParsedMarkdownDocument
    let isActive: Bool
    let jumpRequest: MarkdownPreviewJumpRequest?
    @Binding var selectedOutlineID: String?
    @Binding var readingProgress: CGFloat

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = NSScrollView()
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = false
        scrollView.autohidesScrollers = true
        scrollView.borderType = .noBorder
        scrollView.drawsBackground = true
        scrollView.backgroundColor = AppKitThemeAdapter.editor
        scrollView.scrollerStyle = .overlay
        scrollView.contentView.postsBoundsChangedNotifications = true

        let textStorage = NSTextStorage()
        let layoutManager = MarkdownPreviewLayoutManager(
            metrics: .current
        )
        layoutManager.allowsNonContiguousLayout = true
        textStorage.addLayoutManager(layoutManager)
        let textContainer = NSTextContainer(
            containerSize: NSSize(
                width: scrollView.contentSize.width,
                height: CGFloat.greatestFiniteMagnitude
            )
        )
        textContainer.widthTracksTextView = true
        textContainer.lineFragmentPadding = 0
        layoutManager.addTextContainer(textContainer)

        let textView = MarkdownPreviewTextView(
            frame: NSRect(origin: .zero, size: scrollView.contentSize),
            textContainer: textContainer
        )
        textView.minSize = NSSize(width: 0, height: scrollView.contentSize.height)
        textView.maxSize = NSSize(
            width: CGFloat.greatestFiniteMagnitude,
            height: CGFloat.greatestFiniteMagnitude
        )
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.autoresizingMask = [.width]
        textView.isEditable = false
        textView.isSelectable = true
        textView.isRichText = true
        textView.importsGraphics = false
        textView.usesFontPanel = false
        textView.usesRuler = false
        textView.usesFindBar = true
        textView.isIncrementalSearchingEnabled = true
        textView.allowsUndo = false
        textView.drawsBackground = true
        textView.backgroundColor = AppKitThemeAdapter.editor
        textView.insertionPointColor = AppKitThemeAdapter.accent
        textView.selectedTextAttributes = [
            .backgroundColor: AppKitThemeAdapter.selection
        ]
        textView.linkTextAttributes = [
            .foregroundColor: AppKitThemeAdapter.accent,
            .underlineStyle: NSUnderlineStyle.single.rawValue,
            .underlineColor: MarkdownLinkStylePolicy.normalUnderlineColor
        ]
        textView.setAccessibilityLabel("Markdown preview")
        scrollView.documentView = textView
        context.coordinator.attach(scrollView: scrollView, textView: textView)
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        context.coordinator.update(
            document: document,
            scale: scale,
            displayScale: displayScale,
            usesDarkAppearance: colorScheme == .dark,
            isActive: isActive,
            jumpRequest: jumpRequest,
            onSelectedOutlineChange: { selectedOutlineID = $0 },
            onReadingProgressChange: { readingProgress = $0 }
        )
    }

    func sizeThatFits(
        _ proposal: ProposedViewSize,
        nsView: NSScrollView,
        context: Context
    ) -> CGSize? {
        CGSize(
            width: proposal.width ?? nsView.frame.width,
            height: proposal.height ?? nsView.frame.height
        )
    }

    static func dismantleNSView(_ scrollView: NSScrollView, coordinator: Coordinator) {
        coordinator.stop()
    }

    @MainActor
    final class Coordinator {
        private static let highlightService = SyntaxHighlightService()

        private weak var scrollView: NSScrollView?
        private weak var textView: NSTextView?
        private var boundsObserver: NSObjectProtocol?
        private var renderedSource = ""
        private var renderedDirectoryURL: URL?
        private var renderedScale: CGFloat = 0
        private var renderedDisplayScale: CGFloat = 0
        private var renderedDarkAppearance: Bool?
        private var headings: [MarkdownAttributedHeading] = []
        private var codeBlocks: [MarkdownCodeBlockRegion] = []
        private var imageFigures: [MarkdownImageFigureRegion] = []
        private var mermaidFigures: [MarkdownMermaidFigureRegion] = []
        private var codeCopyControls: [String: NSHostingView<MarkdownCodeCopyControl>] = [:]
        private var mermaidControls: [String: NSHostingView<MarkdownMermaidSourceControl>] = [:]
        private let mermaidExpansion = MarkdownMermaidSourceExpansion()
        private var appliedJumpRequest: MarkdownPreviewJumpRequest?
        private var lastReportedOutlineID: String?
        private var onSelectedOutlineChange: (String?) -> Void = { _ in }
        private var onReadingProgressChange: (CGFloat) -> Void = { _ in }
        private var highlightGeneration = 0
        private var highlightTask: Task<Void, Never>?
        private var imageGeneration = 0
        private var imageTask: Task<Void, Never>?
        private var mermaidGeneration = 0
        private var mermaidTask: Task<Void, Never>?
        private var lastReportedProgressPixel = -1
        private var lastViewportWidth: CGFloat = 0
        private var containerMeasure: CGFloat = 0
        private var renderedMeasureBucket: CGFloat = -1
        private var isActive = false

        func attach(scrollView: NSScrollView, textView: NSTextView) {
            self.scrollView = scrollView
            self.textView = textView
            boundsObserver = NotificationCenter.default.addObserver(
                forName: NSView.boundsDidChangeNotification,
                object: scrollView.contentView,
                queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated {
                    self?.updateTextInsets()
                    self?.syncActiveHeading()
                    self?.syncReadingProgress()
                    self?.syncVisibleOverlayControls()
                }
            }
        }

        func update(
            document: ParsedMarkdownDocument,
            scale: CGFloat,
            displayScale: CGFloat,
            usesDarkAppearance: Bool,
            isActive: Bool,
            jumpRequest: MarkdownPreviewJumpRequest?,
            onSelectedOutlineChange: @escaping (String?) -> Void,
            onReadingProgressChange: @escaping (CGFloat) -> Void
        ) {
            self.onSelectedOutlineChange = onSelectedOutlineChange
            self.onReadingProgressChange = onReadingProgressChange
            updateActiveState(isActive)
            updateTextInsets()

            let needsRender =
                renderedSource != document.source
                || renderedDirectoryURL != document.sourceDirectoryURL
                || renderedScale != scale
                || renderedDisplayScale != displayScale
                || renderedDarkAppearance != usesDarkAppearance
                || renderedMeasureBucket != MarkdownBleedPolicy.bucketed(containerMeasure)
            if needsRender {
                renderedSource = document.source
                renderedDirectoryURL = document.sourceDirectoryURL
                renderedScale = scale
                renderedDisplayScale = displayScale
                renderedDarkAppearance = usesDarkAppearance
                renderedMeasureBucket = MarkdownBleedPolicy.bucketed(containerMeasure)
                let rendered = MarkdownAttributedDocumentBuilder.build(
                    document: document,
                    scale: scale,
                    displayScale: displayScale,
                    usesDarkAppearance: usesDarkAppearance,
                    containerMeasure: containerMeasure > 0 ? containerMeasure : nil
                )
                apply(rendered)
                scheduleHighlights(
                    rendered.codeHighlights,
                    usesDarkAppearance: usesDarkAppearance
                )
                scheduleImageLoads(rendered.imageFigures)
                scheduleMermaidRenders(rendered.mermaidFigures)
            }

            if appliedJumpRequest != jumpRequest {
                appliedJumpRequest = jumpRequest
                if let jumpRequest {
                    scrollToHeading(id: jumpRequest.outlineID)
                }
            }
        }

        func stop() {
            highlightGeneration += 1
            highlightTask?.cancel()
            highlightTask = nil
            imageGeneration += 1
            imageTask?.cancel()
            imageTask = nil
            mermaidGeneration += 1
            mermaidTask?.cancel()
            mermaidTask = nil
            for control in codeCopyControls.values {
                control.removeFromSuperview()
            }
            codeCopyControls.removeAll(keepingCapacity: false)
            for control in mermaidControls.values {
                control.removeFromSuperview()
            }
            mermaidControls.removeAll(keepingCapacity: false)
            mermaidExpansion.reset()
            codeBlocks = []
            imageFigures = []
            mermaidFigures = []
            if let boundsObserver {
                NotificationCenter.default.removeObserver(boundsObserver)
            }
            boundsObserver = nil
            textView = nil
            scrollView = nil
            onSelectedOutlineChange = { _ in }
            onReadingProgressChange = { _ in }
        }

        private func updateActiveState(_ active: Bool) {
            let becameInactive = isActive && !active
            isActive = active
            guard let scrollView, let textView else { return }
            scrollView.isHidden = !active
            textView.isSelectable = active
            if becameInactive,
               let window = textView.window,
               let responder = window.firstResponder,
               responder === textView || (responder as? NSView)?.isDescendant(of: scrollView) == true {
                window.makeFirstResponder(nil)
            }
        }

        private func updateTextInsets() {
            guard let scrollView, let textView else { return }
            let width = scrollView.contentView.bounds.width
            guard width > 0, width != lastViewportWidth else { return }
            lastViewportWidth = width
            // Centre the container on the bleed measure, not the prose measure.
            // Prose is held narrower by its own inset inside that container.
            let horizontal = max(
                AtelierMetrics.spaceXL,
                (width - AtelierMetrics.documentBleedMaxWidth) / 2
            )
            textView.textContainerInset = NSSize(
                width: horizontal,
                height: AtelierMetrics.space2XL
            )
            containerMeasure = max(0, width - horizontal * 2)
        }

        private func apply(_ document: MarkdownAttributedDocument) {
            guard let scrollView,
                  let textView,
                  let textStorage = textView.textStorage else { return }
            let origin = scrollView.contentView.bounds.origin
            let selection = textView.selectedRange()
            headings = document.headings
            codeBlocks = document.codeBlocks
            imageFigures = document.imageFigures
            mermaidFigures = document.mermaidFigures
            mermaidExpansion.reset()
            let validCodeBlockIDs = Set(codeBlocks.map(\.id))
            let staleCodeBlockIDs = codeCopyControls.keys.filter {
                !validCodeBlockIDs.contains($0)
            }
            for id in staleCodeBlockIDs {
                codeCopyControls[id]?.removeFromSuperview()
                codeCopyControls[id] = nil
            }
            for block in codeBlocks {
                codeCopyControls[block.id]?.rootView = MarkdownCodeCopyControl(
                    source: block.source
                )
            }
            let validMermaidIDs = Set(mermaidFigures.map(\.id))
            let staleMermaidIDs = mermaidControls.keys.filter {
                !validMermaidIDs.contains($0)
            }
            for id in staleMermaidIDs {
                mermaidControls[id]?.removeFromSuperview()
                mermaidControls[id] = nil
            }
            textStorage.setAttributedString(document.attributedString)
            (textView as? MarkdownPreviewTextView)?.resetHoveredLink()
            let length = textStorage.length
            if selection.location <= length {
                textView.setSelectedRange(
                    NSRange(
                        location: selection.location,
                        length: min(selection.length, length - selection.location)
                    )
                )
            }
            if let textContainer = textView.textContainer {
                textView.layoutManager?.ensureLayout(for: textContainer)
            }
            let maximumY = max(
                0,
                textView.frame.height - scrollView.contentView.bounds.height
            )
            scrollView.contentView.scroll(
                to: NSPoint(x: 0, y: min(maximumY, max(0, origin.y)))
            )
            scrollView.reflectScrolledClipView(scrollView.contentView)
            syncActiveHeading()
            syncReadingProgress()
            syncVisibleOverlayControls()
        }

        private func scheduleImageLoads(
            _ figures: [MarkdownImageFigureRegion]
        ) {
            imageGeneration += 1
            let generation = imageGeneration
            imageTask?.cancel()
            imageTask = nil
            guard !figures.isEmpty else { return }
            imageTask = Task { [weak self] in
                for figure in figures {
                    guard !Task.isCancelled else { return }
                    guard let decoded = await MarkdownLocalImageLoader.load(
                        figure.url
                    ) else {
                        continue
                    }
                    guard !Task.isCancelled else { return }
                    self?.applyImage(
                        decoded,
                        to: figure,
                        generation: generation
                    )
                }
            }
        }

        private func applyImage(
            _ decoded: MarkdownDecodedImage,
            to figure: MarkdownImageFigureRegion,
            generation: Int
        ) {
            guard generation == imageGeneration else { return }
            let bounds = MarkdownImageFigureLayout.fittedBounds(
                pixelWidth: decoded.width,
                pixelHeight: decoded.height
            )
            applyFigure(
                image: MarkdownImageFigureRenderer.image(
                    size: bounds.size,
                    content: decoded.cgImage()
                ),
                bounds: bounds,
                attachment: figure.attachment,
                range: currentRange(for: figure)
            )
        }

        private func scheduleMermaidRenders(
            _ figures: [MarkdownMermaidFigureRegion]
        ) {
            mermaidGeneration += 1
            let generation = mermaidGeneration
            mermaidTask?.cancel()
            mermaidTask = nil
            guard !figures.isEmpty else { return }
            let width = MarkdownMermaidFigureLayout.renderWidth()
            mermaidTask = Task { @MainActor [weak self] in
                for figure in figures {
                    guard !Task.isCancelled else { return }
                    do {
                        let rendered = try await MermaidImageCache.shared.image(
                            source: figure.source,
                            width: width
                        )
                        guard !Task.isCancelled else { return }
                        self?.applyMermaid(
                            rendered,
                            to: figure,
                            generation: generation
                        )
                    } catch is CancellationError {
                        // An evicted queue slot is not a render failure: keep the
                        // placeholder so a later pass can still draw the diagram.
                        return
                    } catch {
                        guard !Task.isCancelled else { return }
                        self?.applyMermaidFailure(
                            to: figure,
                            generation: generation
                        )
                    }
                }
            }
        }

        private func applyMermaid(
            _ rendered: NSImage,
            to figure: MarkdownMermaidFigureRegion,
            generation: Int
        ) {
            guard generation == mermaidGeneration else { return }
            let bounds = MarkdownMermaidFigureLayout.fittedBounds(
                imageSize: rendered.size
            )
            applyFigure(
                image: MarkdownImageFigureRenderer.image(
                    size: bounds.size,
                    content: rendered.cgImage(
                        forProposedRect: nil,
                        context: nil,
                        hints: nil
                    )
                ),
                bounds: bounds,
                attachment: figure.attachment,
                range: currentRange(for: figure)
            )
        }

        private func applyMermaidFailure(
            to figure: MarkdownMermaidFigureRegion,
            generation: Int
        ) {
            guard generation == mermaidGeneration else { return }
            let bounds = figure.attachment.bounds
            applyFigure(
                image: MarkdownImageFigureRenderer.image(
                    size: bounds.size,
                    content: nil,
                    message: MarkdownMermaidFigureLayout.failureMessage
                ),
                bounds: bounds,
                attachment: figure.attachment,
                range: currentRange(for: figure)
            )
        }

        /// Swaps one attachment in place: re-layout only that character range and
        /// hold the reader's scroll origin while the figure changes height.
        private func applyFigure(
            image: NSImage,
            bounds: NSRect,
            attachment: NSTextAttachment,
            range: NSRange
        ) {
            guard let scrollView,
                  let textView,
                  let layoutManager = textView.layoutManager,
                  let textStorage = textView.textStorage,
                  NSMaxRange(range) <= textStorage.length else {
                return
            }
            let origin = scrollView.contentView.bounds.origin
            attachment.image = image
            attachment.bounds = bounds
            layoutManager.invalidateLayout(
                forCharacterRange: range,
                actualCharacterRange: nil
            )
            layoutManager.invalidateDisplay(forCharacterRange: range)
            textView.needsDisplay = true
            scrollView.contentView.scroll(to: origin)
            scrollView.reflectScrolledClipView(scrollView.contentView)
        }

        private func syncVisibleOverlayControls() {
            guard let scrollView,
                  let textView,
                  !(codeBlocks.isEmpty && mermaidFigures.isEmpty),
                  let visibleCharacterRange = MarkdownOverlayControlLayout
                      .visibleCharacterRange(
                          in: textView,
                          visibleRect: scrollView.contentView.bounds
                      ) else {
                removeOverlayControls()
                return
            }
            let visibleBlocks = codeBlocks.filter {
                NSIntersectionRange($0.headerRange, visibleCharacterRange).length > 0
            }
            let visibleBlockIDs = Set(visibleBlocks.map(\.id))
            for id in codeCopyControls.keys.filter({ !visibleBlockIDs.contains($0) }) {
                codeCopyControls[id]?.removeFromSuperview()
                codeCopyControls[id] = nil
            }
            for block in visibleBlocks {
                let control: NSHostingView<MarkdownCodeCopyControl>
                if let existing = codeCopyControls[block.id] {
                    control = existing
                } else {
                    let created = NSHostingView(
                        rootView: MarkdownCodeCopyControl(source: block.source)
                    )
                    scrollView.addSubview(
                        created,
                        positioned: .above,
                        relativeTo: scrollView.contentView
                    )
                    codeCopyControls[block.id] = created
                    control = created
                }
                place(control, over: block.headerRange, anchor: .centered)
            }

            let visibleFigures = mermaidFigures.filter {
                NSIntersectionRange($0.range, visibleCharacterRange).length > 0
            }
            let visibleFigureIDs = Set(visibleFigures.map(\.id))
            for id in mermaidControls.keys.filter({ !visibleFigureIDs.contains($0) }) {
                mermaidControls[id]?.removeFromSuperview()
                mermaidControls[id] = nil
            }
            for figure in visibleFigures {
                let control: NSHostingView<MarkdownMermaidSourceControl>
                if let existing = mermaidControls[figure.id] {
                    control = existing
                } else {
                    let created = NSHostingView(
                        rootView: mermaidSourceControl(for: figure)
                    )
                    scrollView.addSubview(
                        created,
                        positioned: .above,
                        relativeTo: scrollView.contentView
                    )
                    mermaidControls[figure.id] = created
                    control = created
                }
                place(control, over: figure.range, anchor: .top)
            }
        }

        private func mermaidSourceControl(
            for figure: MarkdownMermaidFigureRegion
        ) -> MarkdownMermaidSourceControl {
            MarkdownMermaidSourceControl(
                isExpanded: mermaidExpansion.isExpanded(figure.id)
            ) { [weak self] in
                self?.toggleMermaidSource(id: figure.id)
            }
        }

        private func place(
            _ control: NSView,
            over range: NSRange,
            anchor: MarkdownOverlayAnchor
        ) {
            guard let scrollView, let textView else { return }
            let size = control.fittingSize
            guard let frame = MarkdownOverlayControlLayout.frame(
                for: range,
                size: size,
                anchor: anchor,
                in: textView,
                host: scrollView
            ) else {
                return
            }
            control.frame = NSRect(
                x: max(scrollView.contentView.frame.minX, frame.minX),
                y: frame.minY,
                width: frame.width,
                height: frame.height
            ).integral
        }

        private func removeOverlayControls() {
            for control in codeCopyControls.values {
                control.removeFromSuperview()
            }
            codeCopyControls.removeAll(keepingCapacity: true)
            for control in mermaidControls.values {
                control.removeFromSuperview()
            }
            mermaidControls.removeAll(keepingCapacity: true)
        }

        private func toggleMermaidSource(id: String) {
            guard let textView,
                  let textStorage = textView.textStorage,
                  let figure = mermaidFigures.first(where: { $0.id == id }) else {
                return
            }
            guard let edit = mermaidExpansion.toggle(
                figure: figure,
                in: textStorage,
                scale: renderedScale,
                displayScale: renderedDisplayScale,
                presentation: .document
            ) else {
                return
            }
            shiftRegions(after: edit.location, by: edit.delta)
            for figure in mermaidFigures {
                mermaidControls[figure.id]?.rootView = mermaidSourceControl(
                    for: figure
                )
            }
            syncVisibleOverlayControls()
        }

        /// An in-place storage edit moves every later range, so the stored
        /// regions have to follow it instead of being rebuilt.
        private func shiftRegions(after location: Int, by delta: Int) {
            guard delta != 0 else { return }
            headings = headings.map {
                MarkdownAttributedHeading(
                    id: $0.id,
                    range: MarkdownRegionShift.shifted(
                        $0.range,
                        after: location,
                        by: delta
                    )
                )
            }
            codeBlocks = codeBlocks.map {
                MarkdownCodeBlockRegion(
                    id: $0.id,
                    headerRange: MarkdownRegionShift.shifted(
                        $0.headerRange,
                        after: location,
                        by: delta
                    ),
                    sourceRange: MarkdownRegionShift.shifted(
                        $0.sourceRange,
                        after: location,
                        by: delta
                    ),
                    source: $0.source,
                    usesGeneratedLineNumbers: $0.usesGeneratedLineNumbers
                )
            }
            imageFigures = imageFigures.map {
                MarkdownImageFigureRegion(
                    id: $0.id,
                    url: $0.url,
                    attachment: $0.attachment,
                    range: MarkdownRegionShift.shifted(
                        $0.range,
                        after: location,
                        by: delta
                    )
                )
            }
            mermaidFigures = mermaidFigures.map {
                MarkdownMermaidFigureRegion(
                    id: $0.id,
                    source: $0.source,
                    attachment: $0.attachment,
                    range: MarkdownRegionShift.shifted(
                        $0.range,
                        after: location,
                        by: delta
                    )
                )
            }
        }

        /// A load started before a toggle carries the range it was scheduled
        /// with, so resolve the current range by id instead of that stale one.
        private func currentRange(
            for figure: MarkdownImageFigureRegion
        ) -> NSRange {
            imageFigures.first { $0.id == figure.id }?.range ?? figure.range
        }

        private func currentRange(
            for figure: MarkdownMermaidFigureRegion
        ) -> NSRange {
            mermaidFigures.first { $0.id == figure.id }?.range ?? figure.range
        }

        private func scheduleHighlights(
            _ requests: [MarkdownCodeHighlightRequest],
            usesDarkAppearance: Bool
        ) {
            highlightGeneration += 1
            let generation = highlightGeneration
            highlightTask?.cancel()
            highlightTask = nil
            guard !requests.isEmpty else { return }

            highlightTask = Task { [weak self] in
                await Task.yield()
                for request in requests {
                    guard !Task.isCancelled else { return }
                    do {
                        let highlighted = try await Self.highlightService.highlightPreservingWhitespace(
                            request.source,
                            languageName: request.languageName,
                            usesDarkAppearance: usesDarkAppearance
                        )
                        guard !Task.isCancelled else { return }
                        self?.applyHighlight(
                            highlighted,
                            to: request.range,
                            generation: generation
                        )
                    } catch {
                        guard !(error is CancellationError) else { return }
                    }
                }
            }
        }

        private func applyHighlight(
            _ highlighted: AttributedString,
            to targetRange: NSRange,
            generation: Int
        ) {
            guard generation == highlightGeneration,
                  let textStorage = textView?.textStorage,
                  NSMaxRange(targetRange) <= textStorage.length else { return }
            let native = NSAttributedString(highlighted)
            textStorage.beginEditing()
            native.enumerateAttribute(
                .foregroundColor,
                in: NSRange(location: 0, length: native.length)
            ) { value, range, _ in
                guard let value, range.location < targetRange.length else { return }
                let availableLength = targetRange.length - range.location
                let destination = NSRange(
                    location: targetRange.location + range.location,
                    length: min(range.length, availableLength)
                )
                guard destination.length > 0,
                      NSMaxRange(destination) <= NSMaxRange(targetRange) else { return }
                textStorage.addAttribute(.foregroundColor, value: value, range: destination)
            }
            textStorage.endEditing()
        }

        private func scrollToHeading(id: String) {
            guard let heading = headings.first(where: { $0.id == id }),
                  let scrollView,
                  let textView,
                  let textContainer = textView.textContainer,
                  let layoutManager = textView.layoutManager else { return }
            layoutManager.ensureLayout(forCharacterRange: heading.range)
            let glyphRange = layoutManager.glyphRange(
                forCharacterRange: heading.range,
                actualCharacterRange: nil
            )
            let rect = layoutManager.boundingRect(
                forGlyphRange: glyphRange,
                in: textContainer
            )
            let targetY = max(
                0,
                rect.minY + textView.textContainerOrigin.y - AtelierMetrics.spaceL
            )
            let maximumY = max(
                0,
                textView.frame.height - scrollView.contentView.bounds.height
            )
            scrollView.contentView.scroll(
                to: NSPoint(x: 0, y: min(maximumY, targetY))
            )
            scrollView.reflectScrolledClipView(scrollView.contentView)
            reportActiveHeading(id)
        }

        private func syncActiveHeading() {
            guard isActive,
                  !headings.isEmpty,
                  let scrollView,
                  let textView,
                  let textContainer = textView.textContainer,
                  let layoutManager = textView.layoutManager else { return }
            let origin = textView.textContainerOrigin
            let point = NSPoint(
                x: 1,
                y: max(0, scrollView.contentView.bounds.minY + 64 - origin.y)
            )
            let characterIndex = layoutManager.characterIndex(
                for: point,
                in: textContainer,
                fractionOfDistanceBetweenInsertionPoints: nil
            )
            let active = headings.last(where: { $0.range.location <= characterIndex })
                ?? headings.first
            reportActiveHeading(active?.id)
        }

        private func syncReadingProgress() {
            guard isActive,
                  let scrollView,
                  let textView else {
                return
            }
            let viewportHeight = scrollView.contentView.bounds.height
            let progress = MarkdownReadingProgressPolicy.fraction(
                originY: scrollView.contentView.bounds.minY,
                viewportHeight: viewportHeight,
                documentHeight: textView.frame.height
            )
            let pixel = MarkdownReadingProgressPolicy.visiblePixel(
                fraction: progress,
                railHeight: viewportHeight
            )
            guard pixel != lastReportedProgressPixel else { return }
            lastReportedProgressPixel = pixel
            Task { @MainActor [onReadingProgressChange] in
                onReadingProgressChange(progress)
            }
        }

        private func reportActiveHeading(_ id: String?) {
            guard lastReportedOutlineID != id else { return }
            lastReportedOutlineID = id
            Task { @MainActor [onSelectedOutlineChange] in
                onSelectedOutlineChange(id)
            }
        }
    }
}
