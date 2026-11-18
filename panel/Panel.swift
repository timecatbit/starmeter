import AppKit
import CoreGraphics
import Darwin
import ServiceManagement
import UserNotifications

struct Entry: Decodable {
    var label:String;var separator:Bool?;var image:String?;var template:Bool?;var symbol:String?
    var section:Bool?;var attention:Bool?;var checked:Bool?;var detail:String?;var value:String?;var fallbacks:[String]?;var usageLevel:String?;var statusLight:String?;var radio:Bool?;var href:String?;var action:[String]?;var onOpenAction:[String]?;var children:[Entry]
    var actionable:Bool { !children.isEmpty || href != nil || action != nil }
    var isModule:Bool {action?.first=="set-module"}
    var key:String {if isModule,let action{return action.prefix(2).joined(separator:"\u{001F}")}
        return action?.joined(separator:"\u{001F}") ?? href ?? label.components(separatedBy:" · ").first ?? label}
}
struct ModelUsage:Decodable {var name:String;var tokens:Double;var share:Double;var credits:Double?;var priceNote:String?}
struct CountedUsage:Decodable {var cost:Double?;var tokens:Double;var requests:Int;var unpriced:Int;var cache:Double?;var history:[Double]}
struct ProviderUsage:Decodable {
    var id:String;var name:String;var image:String?;var template:Bool?;var symbol:String?;var subtitle:String;var status:String;var available:Bool
    var cost:Double?;var credits:Double?;var tokens:Double;var requests:Int;var output:Double;var cache:Double?;var coverage:Double?
    var sessions:Int;var unpriced:Int;var summaryIncluded:Bool?;var counted:CountedUsage?;var models:[ModelUsage];var clients:[ModelUsage]?;var history:[Double]
    // A card that groups by model or project takes the colour of the tool or brand it belongs to.
    var accent:String?
    var colour:String {accent ?? id}
    // Model and project cards gather several tools, whose sessions don't add up.
    var grouped:Bool {id.hasPrefix("model:") || id.hasPrefix("project:")}
    // "N 次未定价" when some requests have no price yet, so the cost reads as covering only the rest.
    var priceNote:String?
    // The service's own status page reports trouble right now (服务状态 on).
    var serviceNote:String?
    // What Summary adds for this source: nothing when it isn't counted, and only the requests no gateway
    // logged when a gateway (OpenCodex, Magpie) counts the rest. The card itself keeps every record.
    var summaryPart:ProviderUsage? {
        guard summaryIncluded != false else{return nil}
        guard let c=counted else{return self}
        var part=self;part.cost=c.cost;part.tokens=c.tokens;part.requests=c.requests;part.unpriced=c.unpriced;part.cache=c.cache;part.history=c.history
        return part
    }
}
// The squares under Summary for the selected range, as Summary counts them, in reading order: a row of hours for
// a day, a row of hours per day for a week, and days otherwise. The last `future` squares are still to come.
struct UsageHeatmap:Decodable {
    var columns:Int;var rowLabels:[String]?;var labels:[String];var tokens:[Double];var requests:[Int];var costs:[Double];var unpriced:[Int]
    var future:Int;var hint:String;var status:String?;var notice:String?
    // Days in a row with usage, for ranges of several days: "连续 12 天 · 最长 35 天".
    var streak:String?
    var rows:Int {max(1,(tokens.count+max(1,columns)-1)/max(1,columns))}
    var past:Int {max(0,tokens.count-future)}
    var active:[Bool] {tokens.indices.map{tokens[$0]>0 || (requests.indices.contains($0) && requests[$0]>0)}}
    // Four shades by rank among the squares with tokens, so a few heavy ones don't wash out the rest and the
    // busiest is always the darkest. Squares with requests but no tokens (Qoder CN Credits) take the lightest.
    var levels:[Int] {
        let values=tokens.filter{$0>0}.sorted(),flags=active
        return tokens.indices.map{i in
            guard flags[i] else{return 0}
            guard tokens[i]>0 else{return 1}
            var low=0,high=values.count
            while low<high {let mid=(low+high)/2;if values[mid]<=tokens[i]{low=mid+1}else{high=mid}}
            return max(1,min(4,Int(ceil(4*Double(low)/Double(values.count)))))
        }
    }
    // The square pointed at, in one line: when, tokens, known cost and requests.
    func detail(_ index:Int)->String {
        guard labels.indices.contains(index) else{return ""}
        let n=requests.indices.contains(index) ? requests[index] : 0
        guard tokens.indices.contains(index),active[index] else{return labels[index]+" · 无用量"}
        var parts=[labels[index]]
        if tokens[index]>0 {parts.append(tokenText(tokens[index])+" Token")}
        let cost=costs.indices.contains(index) ? costs[index] : 0,partial=unpriced.indices.contains(index) && unpriced[index]>0
        if cost>0 {parts.append((partial ? "已知 " : "")+money(cost))}
        parts.append("\(n) 请求")
        return parts.joined(separator:" · ")
    }
    // Said at the bottom left while no square is pointed at: why there's nothing yet, or what is missing.
    var note:String? {
        if let status,!status.isEmpty {return status}
        if !active.contains(true) {return "暂无用量"}
        if let notice,!notice.isEmpty {return notice}
        return streak
    }
}
// Where each square of a heatmap goes in a given width. Every square is the same size in every range; the space
// between squares stretches so each row runs the full width, whether it holds 24 hours or 31 days, and rows sit
// that same space apart, so the spacing is even every way.
struct HeatmapLayout {
    // The panel's square; share pictures, which are wider, use a slightly larger one so the spacing reads the same.
    static let cell:CGFloat=8,rowGap:CGFloat=1.6
    let origin:NSPoint,width:CGFloat,columns:Int,rows:Int
    var size:CGFloat=HeatmapLayout.cell
    var columnPitch:CGFloat {columns>1 ? (width-size)/CGFloat(columns-1) : 0}
    var gap:CGFloat {columns>1 ? columnPitch-size : Self.rowGap}
    var rowPitch:CGFloat {size+gap}
    var frame:NSRect {NSRect(x:origin.x,y:origin.y,width:width,height:CGFloat(rows)*rowPitch-gap)}
    func square(_ index:Int)->NSRect {
        NSRect(x:origin.x+CGFloat(index%max(1,columns))*columnPitch,y:origin.y+CGFloat(index/max(1,columns))*rowPitch,width:size,height:size)
    }
    // The square nearest a point, so the space between squares belongs to them too.
    func index(at point:NSPoint)->Int? {
        guard columns>0,frame.insetBy(dx:-1,dy:-1).contains(point) else{return nil}
        let column=columns>1 ? Int(((point.x-origin.x-size/2)/columnPitch).rounded()) : 0,row=Int((point.y-origin.y)/rowPitch)
        return min(rows-1,max(0,row))*columns+min(columns-1,max(0,column))
    }
}
struct Grouping:Decodable {var key:String;var label:String}
struct Dashboard:Decodable {var date:String;var dates:[String];var providers:[ProviderUsage];var rangeKey:String;var rangeLabel:String;var chartUnit:String;var chartSpan:String;var period:[String]?;var chartStatus:String;var updated:String?;var chartPositions:[Double]?;var heatmap:UsageHeatmap?;var showSummary:Bool?;var allowCollapse:Bool?;var expandedProviders:[String]?
    // The Harness cards: one per tool, or regrouped by model or project. Summary, the chart and the share picture stay per tool.
    var grouping:String?;var groupings:[Grouping]?;var cards:[ProviderUsage]?
    var currency:MoneyUnit?
    // How the regrouped cards count, when that differs from Summary (projects come from each client's own records).
    var groupNote:String?
    var cardList:[ProviderUsage] {cards ?? providers}
    // Regrouped cards always show their rows; folding is per tool.
    var collapsible:Bool {allowCollapse==true && cards==nil}
    // Part way from an earlier reading to this one: the figures, the chart's history and the model shares move,
    // the layout (which cards, which rows) is this one's.
    static func blend(_ a:Dashboard,_ b:Dashboard,_ t:Double)->Dashboard {
        func mix(_ list:[ProviderUsage],_ old:[ProviderUsage])->[ProviderUsage] {list.map{p in old.first{$0.id==p.id}.map{ProviderUsage.blend($0,p,t)} ?? p}}
        var out=b;out.providers=mix(b.providers,a.providers)
        if let cards=b.cards {out.cards=mix(cards,a.cards ?? [])}
        return out
    }
    // The same readings, or only text changed: nothing to animate.
    static func moved(_ a:Dashboard,_ b:Dashboard)->Bool {
        func key(_ list:[ProviderUsage])->[Double] {list.flatMap{[$0.tokens,$0.cost ?? 0,Double($0.requests),$0.output]+$0.history}}
        return key(a.providers) != key(b.providers) || key(a.cards ?? []) != key(b.cards ?? [])
    }
}
extension ProviderUsage {
    static func blend(_ a:ProviderUsage,_ b:ProviderUsage,_ t:Double)->ProviderUsage {
        func mix(_ x:Double,_ y:Double)->Double {x+(y-x)*t}
        func count(_ x:Int,_ y:Int)->Int {Int((Double(x)+Double(y-x)*t).rounded())}
        func series(_ x:[Double],_ y:[Double])->[Double] {x.count==y.count ? zip(x,y).map{mix($0,$1)} : y}
        var p=b
        p.tokens=mix(a.tokens,b.tokens);p.output=mix(a.output,b.output);p.requests=count(a.requests,b.requests);p.history=series(a.history,b.history)
        if let x=a.cost,let y=b.cost {p.cost=mix(x,y)}
        if let x=a.credits,let y=b.credits {p.credits=mix(x,y)}
        if let x=a.counted,var y=b.counted {
            y.tokens=mix(x.tokens,y.tokens);y.requests=count(x.requests,y.requests);y.history=series(x.history,y.history)
            if let c=x.cost,let d=y.cost {y.cost=mix(c,d)};p.counted=y
        }
        p.models=b.models.map{m in a.models.first{$0.name==m.name}.map{old in var n=m;n.tokens=mix(old.tokens,m.tokens);n.share=mix(old.share,m.share);return n} ?? m}
        return p
    }
}
struct Project:Decodable {var name:String;var url:String}
struct Snapshot:Decodable {var project:Project?;var menuBarText:String?;var pinInterval:Double?;var dashboard:Dashboard?;var entries:[Entry];var rangeOptions:[Entry]?;var updateCount:Int?;var moduleOptions:[Entry]?;var appearance:String?;var theme:String?;var preferences:PresentationPreferences?}
func dark(_ view:NSView)->Bool {view.effectiveAppearance.bestMatch(from:[.darkAqua,.aqua]) == .darkAqua}
// The panel's own accent, as magpie's: a soft indigo, periwinkle in the dark, in step with the night-sky mark. It marks
// what is chosen or on (switches, segments, bars, the pinned ring) and nothing else; actions keep the text colour.
let starAccent=NSColor(name:"starAccent"){appearance in
    appearance.bestMatch(from:[.darkAqua,.aqua]) == .darkAqua ? NSColor(srgbRed:0.647,green:0.706,blue:0.988,alpha:1) : NSColor(srgbRed:0.345,green:0.31,blue:0.89,alpha:1)
}
func background(_ view:NSView)->NSColor {dark(view) ? NSColor(calibratedWhite:0.115,alpha:1) : NSColor(calibratedWhite:0.992,alpha:1)}
func accent(_ id:String,_ view:NSView)->NSColor {
    if id=="claude" {return dark(view) ? NSColor(srgbRed:0.91,green:0.65,blue:0.46,alpha:1) : NSColor(srgbRed:0.75,green:0.40,blue:0.25,alpha:1)}
    if id=="codex" {return .systemPurple}
    if id=="opencode" {return .systemPink}
    if id=="workbuddy" {return .systemOrange}
    if id=="qoder" {return .systemGreen}
    if id=="gemini" {return .systemTeal}
    if id=="deepseek-harness" {return .systemIndigo}
    if id=="magpie" {return .systemYellow}
    if id=="cursor" {return dark(view) ? NSColor(srgbRed:0.70,green:0.74,blue:0.80,alpha:1) : NSColor(srgbRed:0.33,green:0.37,blue:0.43,alpha:1)}
    return dark(view) ? NSColor(srgbRed:0.55,green:0.71,blue:0.94,alpha:1) : NSColor(srgbRed:0.32,green:0.51,blue:0.78,alpha:1)
}
// A tool's mark on its card, in the logo's own colours: monochrome logos and symbols in the text colour,
// colour logos (Claude, Gemini, DeepSeek) as they are. Without one, the tool's chart-coloured dot.
enum ToolMark {
    private static var cache:[String:NSImage]=[:]
    // Where a line of text drawn from `top` looks centred: halfway up its capitals, which is where a
    // square logo beside the text sits level with it.
    static func middle(of font:NSFont,top:CGFloat)->CGFloat {top+font.ascender-font.capHeight/2}
    static func image(_ p:ProviderUsage)->NSImage? {
        let key=p.id+"|"+(p.image.map{String($0.count)+String($0.suffix(16))} ?? p.symbol ?? "")
        if let cached=cache[key]{return cached}
        var glyph:NSImage?
        if let raw=p.image,let data=Data(base64Encoded:raw),let img=NSImage(data:data){glyph=img;img.isTemplate=p.template ?? false}
        else if let symbol=p.symbol{glyph=NSImage(systemSymbolName:symbol,accessibilityDescription:nil)}
        if let glyph{cache[key]=glyph}
        return glyph
    }
    static func draw(_ p:ProviderUsage,in rect:NSRect,color:NSColor){
        guard let glyph=image(p) else{color.setFill();NSBezierPath(ovalIn:NSRect(x:rect.midX-2.5,y:rect.midY-2.5,width:5,height:5)).fill();return}
        if glyph.isTemplate {
            let tinted=NSImage(size:rect.size);tinted.lockFocus()
            glyph.draw(in:NSRect(origin:.zero,size:rect.size));NSColor.labelColor.setFill();NSRect(origin:.zero,size:rect.size).fill(using:.sourceIn)
            tinted.unlockFocus()
            tinted.draw(in:rect,from:.zero,operation:.sourceOver,fraction:1,respectFlipped:true,hints:nil)
        } else {glyph.draw(in:rect,from:.zero,operation:.sourceOver,fraction:1,respectFlipped:true,hints:nil)}
    }
}
func statusGreen(_ view:NSView)->NSColor {dark(view) ? NSColor(srgbRed:0.49,green:0.79,blue:0.61,alpha:1) : NSColor(srgbRed:0.18,green:0.49,blue:0.31,alpha:1)}
// One status light for every row: new (accent), ok (green), issue (orange), anything else a hollow grey ring.
func drawStatusLight(_ state:String,centre:NSPoint,in view:NSView){
    let dot=NSBezierPath(ovalIn:NSRect(x:centre.x-3.5,y:centre.y-3.5,width:7,height:7))
    switch state {
    case "new":starAccent.setFill();dot.fill()
    case "ok":statusGreen(view).setFill();dot.fill()
    case "issue":NSColor.systemOrange.setFill();dot.fill()
    default:NSColor.secondaryLabelColor.setStroke();dot.lineWidth=1;dot.stroke()
    }
}
func usageColor(_ level:String?)->NSColor? {
    switch level {case "normal":return .labelColor;case "warning":return .systemOrange;case "critical":return .systemRed;default:return nil}
}
func share(_ ratio:Double)->String {let value=ratio*100;return String(format:(value>0 && value<1)||(value>99 && value<100) ? "%.1f%%" : "%.0f%%",value)}
func tokens(_ value:Double)->String {
    if value>=1e9{return String(format:"%.2fB",value/1e9)}
    if value>=1e6{return String(format:"%.2fM",value/1e6)}
    if value>=1e3{return String(format:"%.1fK",value/1e3)}
    return String(format:"%.0f",value)
}
// The same figure where a type's own `tokens` property hides the function.
func tokenText(_ value:Double)->String {tokens(value)}
func axisTokens(_ value:Double)->String {
    for (unit,suffix) in [(1e9,"B"),(1e6,"M"),(1e3,"K")] where value>=unit {
        let scaled=value/unit
        return String(format:scaled>=10 || scaled==floor(scaled) ? "%.0f%@" : "%.1f%@",scaled,suffix)
    }
    return String(format:"%.0f",value)
}
func modelName(_ name:String)->String {
    guard name.hasPrefix("claude-") else{return name}
    let value=name.replacingOccurrences(of:"-20[0-9]{6}$",with:"",options:.regularExpression)
    let parts=value.dropFirst(7).split(separator:"-").map(String.init)
    guard let family=parts.first,["opus","sonnet","haiku","fable"].contains(family) else{return name}
    return family.capitalized+" "+parts.dropFirst().joined(separator:".")
}
// API-equivalent estimates read in dollars, or in yuan at the rate the snapshot carries. Set from each dashboard shown.
struct MoneyUnit:Decodable {var code:String;var symbol:String;var rate:Double;var note:String?}
enum Currency {
    static var unit:MoneyUnit?
    static var symbol:String {unit?.symbol ?? "$"}
    static func convert(_ value:Double)->Double {value*(unit?.rate ?? 1)}
}
func money(_ value:Double?)->String {value.map{Currency.symbol+String(format:"%.2f",Currency.convert($0))} ?? "—"}
// Qoder CN plan requests log Qoder Credits and no tokens; Credits are never shown as dollars or tokens.
func creditsText(_ value:Double)->String {String(format:value>=100 ? "%.0f Credits" : "%.1f Credits",value)}
func headline(_ p:ProviderUsage)->String {
    if p.cost==nil,let credits=p.credits {return creditsText(credits)}
    return money(p.cost)
}
func tokenLine(_ p:ProviderUsage)->String {p.tokens==0 && p.credits != nil ? "Token 未记录" : tokens(p.tokens)+" Token"}
func modelAmount(_ m:ModelUsage)->String {
    if m.tokens==0,let credits=m.credits {return creditsText(credits)}
    return tokens(m.tokens)
}
func text(_ value:String,x:CGFloat,y:CGFloat,w:CGFloat,size:CGFloat=11,color:NSColor = .labelColor,weight:NSFont.Weight = .regular,align:NSTextAlignment = .left,numeric:Bool=false) {
    let style=NSMutableParagraphStyle();style.alignment=align;style.lineBreakMode = .byTruncatingTail
    var font=numeric ? NSFont.monospacedDigitSystemFont(ofSize:size,weight:weight) : NSFont.systemFont(ofSize:size,weight:weight)
    if numeric {let measured=(value as NSString).size(withAttributes:[.font:font]).width;if measured>w {font=NSFont.monospacedDigitSystemFont(ofSize:max(9,size*w/measured),weight:weight)}}
    (value as NSString).draw(in:NSRect(x:x,y:y,width:w,height:size+6),withAttributes:[.font:font,.foregroundColor:color,.paragraphStyle:style])
}
// A figure reads as one number: rounded, even-width digits, with the $ and the K/M/B unit smaller and lighter
// so the digits carry it. Too wide for its slot, it shrinks rather than cuts.
enum Figure {
    static func font(_ size:CGFloat,_ weight:NSFont.Weight)->NSFont {
        let base=NSFont.monospacedDigitSystemFont(ofSize:size,weight:weight)
        return base.fontDescriptor.withDesign(.rounded).flatMap{NSFont(descriptor:$0,size:size)} ?? base
    }
    static func string(_ value:String,size:CGFloat,weight:NSFont.Weight = .semibold,color:NSColor = .labelColor,align:NSTextAlignment = .left)->NSAttributedString {
        let style=NSMutableParagraphStyle();style.alignment=align;style.lineBreakMode = .byClipping
        let out=NSMutableAttributedString();var body=Substring(value)
        let small:[NSAttributedString.Key:Any]=[.font:font(size*0.62,.medium),.foregroundColor:NSColor.secondaryLabelColor,.paragraphStyle:style]
        if let first=body.first,"$¥".contains(first){out.append(NSAttributedString(string:String(first),attributes:small.merging([.baselineOffset:size*0.3]){$1}));body=body.dropFirst()}
        var unit=""
        if let last=body.last,"KMB".contains(last),body.dropLast().last?.isNumber==true{unit=String(last);body=body.dropLast()}
        out.append(NSAttributedString(string:String(body),attributes:[.font:font(size,weight),.foregroundColor:color,.paragraphStyle:style]))
        if !unit.isEmpty{out.append(NSAttributedString(string:unit,attributes:[.font:font(size*0.74,weight),.foregroundColor:color,.paragraphStyle:style]))}
        return out
    }
    static func width(_ value:String,size:CGFloat,weight:NSFont.Weight = .semibold)->CGFloat {ceil(string(value,size:size,weight:weight).size().width)}
    static func draw(_ value:String,x:CGFloat,y:CGFloat,w:CGFloat,size:CGFloat,weight:NSFont.Weight = .semibold,color:NSColor = .labelColor,align:NSTextAlignment = .left){
        let fitted=width(value,size:size,weight:weight)>w ? max(9,size*w/width(value,size:size,weight:weight)) : size
        string(value,size:fitted,weight:weight,color:color,align:align).draw(in:NSRect(x:x,y:y+(size-fitted)*0.7,width:w,height:size+8))
    }
}
func line(_ a:NSPoint,_ b:NSPoint,color:NSColor = .separatorColor,width:CGFloat=0.5) {
    let p=NSBezierPath();p.move(to:a);p.line(to:b);p.lineWidth=width;color.setStroke();p.stroke()
}
class FlippedView:NSView {override var isFlipped:Bool {true}}
// The module grid's backdrop: a click on its blank space, between and around the cells, closes an open sidebar.
final class BlankClickView:FlippedView {
    var clicked:(()->Void)?
    override func acceptsFirstMouse(for event:NSEvent?)->Bool{true}
    override func mouseDown(with event:NSEvent){clicked?()}
}
final class Surface:FlippedView {
    let content:FlippedView
    private let translucent:Bool
    init(frame:NSRect,glass:Bool=true){
        content=FlippedView(frame:NSRect(origin:.zero,size:frame.size))
        translucent=glass && !NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency
        super.init(frame:frame);wantsLayer=true;layer?.cornerRadius=13;layer?.masksToBounds=true
        content.autoresizingMask=[.width,.height]
        if translucent {
            #if compiler(>=6.2)
            if #available(macOS 26.0,*) {
                let effect=NSGlassEffectView(frame:bounds);effect.style = .regular;effect.cornerRadius=13
                effect.autoresizingMask=[.width,.height];effect.contentView=content;addSubview(effect)
                return
            }
            #endif
            let effect=NSVisualEffectView(frame:bounds);effect.material = .popover;effect.blendingMode = .behindWindow
            effect.state = .active;effect.autoresizingMask=[.width,.height];effect.addSubview(content);addSubview(effect)
        } else {addSubview(content)}
    }
    required init?(coder:NSCoder){fatalError()}
    override func viewDidChangeEffectiveAppearance(){super.viewDidChangeEffectiveAppearance();redraw(self)}
    private func redraw(_ view:NSView){view.needsDisplay=true;for child in view.subviews{redraw(child)}}
    override func draw(_ dirtyRect:NSRect){
        if !translucent {background(self).setFill();bounds.fill()}
        let shape=NSBezierPath(roundedRect:bounds.insetBy(dx:0.25,dy:0.25),xRadius:13,yRadius:13)
        NSColor.separatorColor.withAlphaComponent(0.35).setStroke();shape.lineWidth=0.5;shape.stroke()
    }
}
// Section divider on the homepage: a little heavier than a hairline so groups read apart.
final class Divider:NSView {
    override func draw(_ dirtyRect:NSRect){NSColor.labelColor.withAlphaComponent(0.16).setFill();NSBezierPath(roundedRect:bounds,xRadius:1,yRadius:1).fill()}
}
final class Panel:NSPanel {
    var escape:(()->Void)?
    override var canBecomeKey:Bool {true}
    override func keyDown(with event:NSEvent){if event.keyCode==53{escape?()}else{super.keyDown(with:event)}}
}
// A thin knob at the panel's edge, shown while scrolling: no track, and never the wide always-on bar that
// "Show scroll bars: Always" or a connected mouse would otherwise put over the content's right edge.
final class SlimScroller:NSScroller {
    override class var isCompatibleWithOverlayScrollers:Bool {true}
    override class func scrollerWidth(for controlSize:NSControl.ControlSize,scrollerStyle:NSScroller.Style)->CGFloat {8}
    override func drawKnobSlot(in slotRect:NSRect,highlight flag:Bool){}
    override func drawKnob(){
        let knob=rect(for:.knob);guard knob.height>0 else{return}
        NSColor.labelColor.withAlphaComponent(0.28).setFill()
        NSBezierPath(roundedRect:NSRect(x:bounds.width-3.5,y:knob.minY+2,width:3,height:max(0,knob.height-4)),xRadius:1.5,yRadius:1.5).fill()
    }
}
final class ContentScrollView:NSScrollView {
    var needsVerticalScrolling:Bool{(documentView?.frame.height ?? 0)>contentView.bounds.height+0.5}
    override var scrollerStyle:NSScroller.Style {get{.overlay} set{super.scrollerStyle = .overlay}}
    func configureScrolling(){
        if !(verticalScroller is SlimScroller){verticalScroller=SlimScroller()}
        hasHorizontalScroller=false;horizontalScrollElasticity = .none
        hasVerticalScroller=needsVerticalScrolling
        verticalScrollElasticity=needsVerticalScrolling ? .automatic : .none
        if !needsVerticalScrolling{contentView.scroll(to:.zero);reflectScrolledClipView(contentView)}
    }
    // A page goes back on a two-finger swipe to the right, as Safari does.
    var swipedBack:(()->Void)?;private var sideways:CGFloat=0;private var swiped=false
    override func scrollWheel(with event:NSEvent){
        if let swipedBack {
            if event.phase == .began {sideways=0;swiped=false}
            if abs(event.scrollingDeltaX)>abs(event.scrollingDeltaY) {
                sideways+=event.scrollingDeltaX
                if !swiped && sideways>60 {swiped=true;swipedBack()}
                return
            }
        }
        guard needsVerticalScrolling else{return}
        super.scrollWheel(with:event)
    }
}
// The panel never activates the app, and AppKit only shows tooltips for the active app, so header
// buttons report hovering themselves (activeAlways) and the panel draws a short hint below them.
// A press you can feel: the control sinks a little while the button is down and springs back on release, as
// Token Monitor's buttons do. The click itself still fires on release, as AppKit buttons do.
enum Press {
    static func set(_ view:NSView,_ down:Bool,scale:CGFloat){
        guard !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion else{return}
        view.wantsLayer=true;guard let layer=view.layer else{return}
        let b=view.bounds;var target=CATransform3DIdentity
        if down {target=CATransform3DTranslate(target,b.midX,b.midY,0);target=CATransform3DScale(target,scale,scale,1);target=CATransform3DTranslate(target,-b.midX,-b.midY,0)}
        let animation=CABasicAnimation(keyPath:"transform")
        animation.fromValue=NSValue(caTransform3D:layer.presentation()?.transform ?? layer.transform);animation.toValue=NSValue(caTransform3D:target)
        animation.duration=down ? 0.07 : 0.22;animation.timingFunction=CAMediaTimingFunction(controlPoints:0.25,1,0.5,1)
        layer.transform=target;layer.add(animation,forKey:"press")
    }
    /// Runs AppKit's own tracking with the control pressed in for as long as the button is held.
    static func track(_ view:NSView,scale:CGFloat,_ tracking:()->Void){
        set(view,true,scale:scale);tracking();set(view,false,scale:scale)
    }
}
final class HintButton:NSButton {
    var hint="";var hovered:((HintButton,Bool)->Void)?;private var area:NSTrackingArea?;private var inside=false
    // Bare icons, as magpie's header: a soft rounded fill appears under the pointer and deepens while pressed.
    var bare=false {didSet{if bare{isBordered=false;bezelStyle = .regularSquare;needsDisplay=true}}}
    override func draw(_ dirtyRect:NSRect){
        if bare && (inside || isHighlighted) {
            NSColor.labelColor.withAlphaComponent(isHighlighted ? (dark(self) ? 0.16 : 0.11) : (dark(self) ? 0.09 : 0.06)).setFill()
            NSBezierPath(roundedRect:bounds.insetBy(dx:0.5,dy:0.5),xRadius:7,yRadius:7).fill()
        }
        super.draw(dirtyRect)
    }
    override func updateTrackingAreas(){super.updateTrackingAreas();if let area{removeTrackingArea(area)};area=NSTrackingArea(rect:bounds,options:[.mouseEnteredAndExited,.activeAlways,.inVisibleRect],owner:self,userInfo:nil);addTrackingArea(area!)}
    override func mouseEntered(with event:NSEvent){inside=true;needsDisplay=true;hovered?(self,true)}
    override func mouseExited(with event:NSEvent){inside=false;needsDisplay=true;hovered?(self,false)}
    // A click hides the hint first, so it never sits under the menu the click opens.
    override func mouseDown(with event:NSEvent){hovered?(self,false);Press.track(self,scale:0.88){super.mouseDown(with:event)}}
}
// One way to group the Harness cards: plain text, the chosen one bold on a soft pill, a fainter pill under the
// pointer. No frame or dividers, like the panel's other light controls.
final class GroupTab:NSButton {
    override func acceptsFirstMouse(for event:NSEvent?)->Bool{true}
    var chosen=false{didSet{needsDisplay=true;setAccessibilityValue(chosen ? 1 : 0)}}
    var invoke:(()->Void)?;private var hovered=false;private var area:NSTrackingArea?
    static let font=NSFont.systemFont(ofSize:11.5),boldFont=NSFont.systemFont(ofSize:11.5,weight:.semibold),height:CGFloat=22
    static func width(_ label:String)->CGFloat {ceil((label as NSString).size(withAttributes:[.font:boldFont]).width)+20}
    init(label:String){
        super.init(frame:NSRect(x:0,y:0,width:Self.width(label),height:Self.height));title=label;isBordered=false;setButtonType(.momentaryChange)
        target=self;action=#selector(pressed);setAccessibilityLabel(label);setAccessibilityRole(.radioButton)
    }
    required init?(coder:NSCoder){fatalError()}
    @objc private func pressed(){invoke?()}
    override func mouseDown(with event:NSEvent){Press.track(self,scale:0.92){super.mouseDown(with:event)}}
    override func updateTrackingAreas(){super.updateTrackingAreas();if let area{removeTrackingArea(area)};area=NSTrackingArea(rect:bounds,options:[.mouseEnteredAndExited,.activeAlways,.inVisibleRect],owner:self,userInfo:nil);addTrackingArea(area!)}
    override func mouseEntered(with event:NSEvent){hovered=true;needsDisplay=true}
    override func mouseExited(with event:NSEvent){hovered=false;needsDisplay=true}
    override func draw(_ dirtyRect:NSRect){
        let night=dark(self),down=isHighlighted
        if chosen || hovered || down {
            NSColor.labelColor.withAlphaComponent(down ? (night ? 0.2 : 0.12) : chosen ? (night ? 0.15 : 0.08) : (night ? 0.07 : 0.04)).setFill()
            NSBezierPath(roundedRect:bounds,xRadius:bounds.height/2,yRadius:bounds.height/2).fill()
        }
        let attributes:[NSAttributedString.Key:Any]=[.font:chosen ? Self.boldFont : Self.font,.foregroundColor:chosen ? NSColor.labelColor : NSColor.secondaryLabelColor]
        let size=(title as NSString).size(withAttributes:attributes)
        (title as NSString).draw(at:NSPoint(x:(bounds.width-size.width)/2,y:(bounds.height-size.height)/2),withAttributes:attributes)
    }
}
final class DisclosureButton:NSButton {
    override func acceptsFirstMouse(for event:NSEvent?)->Bool{true}
    var invoke:(()->Void)?;var hovered:((Bool)->Void)?;var held:((Bool)->Void)?;private var area:NSTrackingArea?
    init(label:String,frame:NSRect){super.init(frame:frame);title=label;isBordered=false;setButtonType(.momentaryChange);target=self;action=#selector(pressed);setAccessibilityLabel(label)}
    override func mouseDown(with event:NSEvent){held?(true);super.mouseDown(with:event);held?(false)}
    required init?(coder:NSCoder){fatalError()}
    @objc private func pressed(){invoke?()}
    override func draw(_ dirtyRect:NSRect){}
    override func updateTrackingAreas(){super.updateTrackingAreas();if let area{removeTrackingArea(area)};area=NSTrackingArea(rect:bounds,options:[.mouseEnteredAndExited,.activeAlways,.inVisibleRect],owner:self,userInfo:nil);addTrackingArea(area!)}
    override func mouseEntered(with event:NSEvent){hovered?(true)}
    override func mouseExited(with event:NSEvent){hovered?(false)}
}
// The share picture has its own layout, never the panel's glass, because what shows through the
// glass depends on each user's desktop. People pick its style when they copy or share it: a card, or
// a terminal window. It covers the usage that Summary counts; subscriptions, balances and keys never appear.
class ShareCard:FlippedView {
    enum Style:Int,CaseIterable {
        case card,terminal
        var title:String {self == .card ? "卡片风格" : "终端风格"}
    }
    static let width:CGFloat=420
    let dashboard:Dashboard,project:Project
    var rows:[ProviderUsage] {dashboard.providers}
    // Totals, chart and the share bar use what Summary counts; every source with data is listed.
    var counted:[ProviderUsage] {rows.compactMap{$0.summaryPart}}
    var shared:[ProviderUsage] {counted.filter{$0.tokens>0}}
    // The tag at the right of a tool's token line: what Summary leaves out or a gateway counted.
    static func tag(_ p:ProviderUsage)->String? {
        p.summaryIncluded == false ? "未计入 Summary" : (p.counted != nil && !p.subtitle.isEmpty ? p.subtitle : nil)
    }
    // Requests the price tables can't price yet get a line of their own under the stats, read whole, saying what
    // the amount leaves out; the panel card says the same beside its name.
    static func priceLine(_ p:ProviderUsage)->String? {p.priceNote.map{$0+"，金额只含已定价的部分"}}
    var pictureHeight:CGFloat {0}
    // Every share picture has a band of its style's pattern above the content and another below the signature.
    // The top one is 63pt: a long picture is read on a phone filling the screen's width with its top at the top
    // of the screen, and on an iPhone 393pt wide that is 59pt of screen, leaving the island (11pt down, 37pt
    // tall) the same space below as above, so whatever it covers is only pattern. The bottom one clears the home
    // indicator. Patterns also sit beside the estimate, fading toward the figure.
    static let topBand:CGFloat=63,bottomBand:CGFloat=44
    // Which way a patch of pattern fades: from its top edge down, from its bottom edge up, or from its right edge leftward.
    enum Fade {case down,up,left}
    // How strong the pattern is at a point of a patch: full at the edge it grows from, nothing at the far one.
    static func fade(_ fading:Fade,x:CGFloat,y:CGFloat,in rect:NSRect)->CGFloat {
        let t:CGFloat
        switch fading {
        case .down:t=(y-rect.minY)/rect.height
        case .up:t=(rect.maxY-y)/rect.height
        case .left:t=(rect.maxX-x)/rect.width
        }
        return pow(max(0,1-t),0.8)
    }
    // The patterns' grain is fixed per day, so a picture drawn twice looks the same.
    var crownSeed:Double {Double(dashboard.date.unicodeScalars.reduce(0){$0+Int($1.value)}%97)}
    static func grain(_ a:Int,_ b:Int,_ seed:Double)->UInt32 {
        var value=UInt32(truncatingIfNeeded:a&*73856093 ^ b&*19349663 ^ Int(seed)&*83492791)
        value ^= value<<13;value ^= value>>17;value ^= value<<5
        return value
    }
    // The heatmap as the panel shows it, once it has been read and something has been used.
    var heatmap:UsageHeatmap? {dashboard.heatmap.flatMap{($0.status ?? "").isEmpty && $0.active.contains(true) ? $0 : nil}}
    static let heatmapSquare:CGFloat=9
    static func heatmapGridHeight(_ h:UsageHeatmap,width:CGFloat)->CGFloat {HeatmapLayout(origin:.zero,width:width,columns:h.columns,rows:h.rows,size:heatmapSquare).frame.height}
    func drawHeatmap(_ h:UsageHeatmap,x:CGFloat,y:CGFloat,width:CGFloat,shades:[NSColor]){
        let layout=HeatmapLayout(origin:NSPoint(x:x,y:y),width:width,columns:h.columns,rows:h.rows,size:Self.heatmapSquare),levels=h.levels,radius:CGFloat=1.8
        for index in 0..<min(h.tokens.count,h.rows*h.columns) {
            let rect=layout.square(index)
            if index>=h.past {
                let outline=NSBezierPath(roundedRect:rect.insetBy(dx:0.4,dy:0.4),xRadius:radius,yRadius:radius);outline.lineWidth=0.8
                shades[0].setStroke();outline.stroke();continue
            }
            shades[levels[index]].setFill();NSBezierPath(roundedRect:rect,xRadius:radius,yRadius:radius).fill()
        }
    }
    static func make(_ style:Style,_ dashboard:Dashboard,project:Project)->ShareCard {
        Currency.unit=dashboard.currency
        return style == .terminal ? TerminalShareCard(dashboard,project:project) : CardShareCard(dashboard,project:project)
    }
    init(_ dashboard:Dashboard,project:Project){
        // Every source with data, in the panel's order; those Summary doesn't count are tagged, not moved.
        var shown=dashboard
        shown.providers=dashboard.providers.filter{$0.available}
        self.dashboard=shown;self.project=project
        super.init(frame:.zero)
        frame=NSRect(x:0,y:0,width:Self.width,height:pictureHeight)
    }
    required init?(coder:NSCoder){fatalError()}
    // A gateway's clients, wrapped onto as many lines as the picture needs rather than cut off.
    static func clientLines(_ p:ProviderUsage,width:CGFloat,font:NSFont)->[String] {
        guard DashboardView.clientLine(p) != nil,let clients=p.clients else{return []}
        var lines:[String]=[],current="客户端"
        for item in clients.map({$0.name+" "+share($0.share)}) {
            let longer=current+" · "+item
            if (longer as NSString).size(withAttributes:[.font:font]).width>width && current != "客户端" {lines.append(current);current=item}
            else {current=longer}
        }
        return lines+[current]
    }
    static func title(_ d:Dashboard)->String {
        ["today":"今天","yesterday":"昨天","24h":"近 24 小时","7d":"近 7 天","30d":"近 30 天","week":"本周","month":"本月","all":"累计"][d.rangeKey].map{$0+" AI 用量"} ?? d.rangeLabel+" AI 用量"
    }
    func measure(_ value:String,_ font:NSFont)->CGFloat {ceil((value as NSString).size(withAttributes:[.font:font]).width)}
    func draw(_ value:String,_ rect:NSRect,_ font:NSFont,_ color:NSColor,_ align:NSTextAlignment = .left){
        let style=NSMutableParagraphStyle();style.alignment=align;style.lineBreakMode = .byTruncatingTail
        (value as NSString).draw(in:rect,withAttributes:[.font:font,.foregroundColor:color,.paragraphStyle:style])
    }
    // Text centred on a line by its glyph box, so different fonts side by side share one middle.
    func draw(_ value:String,x:CGFloat,middle:CGFloat,width:CGFloat,_ font:NSFont,_ color:NSColor,_ align:NSTextAlignment = .left){
        let height=ceil(font.ascender-font.descender)
        draw(value,NSRect(x:x,y:middle-height/2,width:width,height:height+2),font,color,align)
    }
    func symbol(_ name:String,centre:NSPoint,size:CGFloat,color:NSColor,weight:NSFont.Weight = .semibold){
        let config=NSImage.SymbolConfiguration(pointSize:size,weight:weight).applying(NSImage.SymbolConfiguration(paletteColors:[color]))
        guard let image=NSImage(systemSymbolName:name,accessibilityDescription:nil)?.withSymbolConfiguration(config) else{return}
        let s=image.size
        image.draw(in:NSRect(x:centre.x-s.width/2,y:centre.y-s.height/2,width:s.width,height:s.height),from:.zero,operation:.sourceOver,fraction:1,respectFlipped:true,hints:nil)
    }
    // Dollars with thousands separators: the picture is read at a glance.
    static let dollars:NumberFormatter={let f=NumberFormatter();f.numberStyle = .decimal;f.minimumFractionDigits=2;f.maximumFractionDigits=2;f.locale=Locale(identifier:"en_US");return f}()
    func amount(_ p:ProviderUsage)->String {p.cost.flatMap{Self.dollars.string(from:NSNumber(value:Currency.convert($0)))}.map{Currency.symbol+$0} ?? headline(p)}
    // The app icon signs the picture: its tile only, without the margin the macOS icon grid leaves around it (100 of 1024).
    private static let logo=Bundle.main.image(forResource:"Starmeter")
    func brandMark(_ rect:NSRect){
        guard let logo=Self.logo else{return}
        let margin=logo.size.width*100/1024
        logo.draw(in:rect,from:NSRect(origin:.zero,size:logo.size).insetBy(dx:margin,dy:margin),operation:.sourceOver,fraction:1,respectFlipped:true,hints:nil)
    }
    // PNG for apps that take it, TIFF for older ones; no file URL, so chat apps paste a picture.
    static func pasteboardItem(_ png:Data)->NSPasteboardItem {
        let item=NSPasteboardItem();item.setData(png,forType:.png)
        if let tiff=NSBitmapImageRep(data:png)?.tiffRepresentation{item.setData(tiff,forType:.tiff)}
        return item
    }
    // Rendered at 2x so the picture stays sharp on Retina screens and in chat apps.
    func png()->Data? {
        guard let rep=NSBitmapImageRep(bitmapDataPlanes:nil,pixelsWide:Int(bounds.width*2),pixelsHigh:Int(bounds.height*2),bitsPerSample:8,samplesPerPixel:4,hasAlpha:true,isPlanar:false,colorSpaceName:.deviceRGB,bytesPerRow:0,bitsPerPixel:0) else{return nil}
        rep.size=bounds.size
        (appearance ?? NSAppearance.currentDrawing()).performAsCurrentDrawingAppearance{cacheDisplay(in:bounds,to:rep)}
        return rep.representation(using:.png,properties:[:])
    }
}
// The card style: an opaque card on a soft wash lit by the leading tools' colours.
final class CardShareCard:ShareCard {
    static let margin:CGFloat=22,inset:CGFloat=24,chartHeight:CGFloat=150,modelHeight:CGFloat=24,legendRow:CGFloat=20
    static let clientWidth=width-margin*2-inset*2-32-12,clientFont=NSFont.systemFont(ofSize:10.5)
    static func blockHeight(_ p:ProviderUsage)->CGFloat {92+(priceLine(p)==nil ? 0 : 18)+CGFloat(clientLines(p,width:clientWidth,font:clientFont).count)*18+CGFloat(max(1,p.models.count))*modelHeight}
    private var cardHeight:CGFloat {
        let header:CGFloat=44,hero:CGFloat=88,tiles:CGFloat=88,chart=dashboard.dates.count>1 ? Self.chartHeight+22 : 0
        let share:CGFloat=shared.isEmpty ? 0 : 60+CGFloat((shared.count+2)/3)*Self.legendRow
        let blocks=rows.isEmpty ? 0 : rows.reduce(0){$0+Self.blockHeight($1)+10}-10
        return Self.inset*2+header+hero+tiles+chart+heatmapHeight+share+blocks
    }
    override var pictureHeight:CGFloat {Self.topBand+cardHeight+60+Self.bottomBand}
    private static let heatmapWidth=width-margin*2-inset*2
    private var heatmapHeight:CGFloat {heatmap.map{24+Self.heatmapGridHeight($0,width:Self.heatmapWidth)+24} ?? 0}
    static func day(_ iso:String)->String {
        let parts=iso.split(separator:"-").compactMap{Int($0)}
        return parts.count==3 ? "\(parts[0]) 年 \(parts[1]) 月 \(parts[2]) 日" : iso
    }
    // What the picture covers: today's or yesterday's date, a 24-hour window with its times, or first to last day.
    static func period(_ d:Dashboard)->String {
        func numbers(_ value:String)->[Int] {value.split(whereSeparator:{"-T:".contains($0)}).compactMap{Int($0)}}
        guard d.rangeKey != "today",let bounds=d.period,bounds.count==2 else{return day(d.date)}
        let a=numbers(bounds[0]),b=numbers(bounds[1])
        guard a.count>=3,b.count==a.count else{return day(d.date)}
        if a.count>=5 {return String(format:"%d/%d %02d:%02d – %d/%d %02d:%02d",a[1],a[2],a[3],a[4],b[1],b[2],b[3],b[4])}
        if a[0] != b[0] {return "\(a[0]) 年 \(a[1]) 月 \(a[2]) 日 – \(b[0]) 年 \(b[1]) 月 \(b[2]) 日"}
        if a[1] != b[1] {return "\(a[0]) 年 \(a[1]) 月 \(a[2]) 日 – \(b[1]) 月 \(b[2]) 日"}
        return a[2] != b[2] ? "\(a[0]) 年 \(a[1]) 月 \(a[2]) – \(b[2]) 日" : "\(a[0]) 年 \(a[1]) 月 \(a[2]) 日"
    }
    private func font(_ size:CGFloat,_ weight:NSFont.Weight,rounded:Bool=false)->NSFont {
        let base=NSFont.systemFont(ofSize:size,weight:weight)
        guard rounded,let descriptor=base.fontDescriptor.withDesign(.rounded) else{return base}
        return NSFont(descriptor:descriptor,size:size) ?? base
    }
    // Large figures shrink to fit their slot rather than truncate.
    private func fitted(_ value:String,_ size:CGFloat,_ weight:NSFont.Weight,room:CGFloat)->NSFont {
        let full=font(size,weight,rounded:true),measured=measure(value,full)
        return measured<=room ? full : font(max(11,floor(size*room/measured)),weight,rounded:true)
    }
    private var teal:NSColor {dark(self) ? NSColor(srgbRed:0.43,green:0.79,blue:0.75,alpha:1) : NSColor(srgbRed:0.15,green:0.47,blue:0.46,alpha:1)}
    // The card's pattern: a halftone of soft dots swelling and shrinking in slow diagonal waves. The waves follow
    // the picture rather than the patch, so every patch of them belongs to the same field.
    private func drawHalftone(in rect:NSRect,fading:Fade,colour:NSColor){
        let pitch:CGFloat=9,seed=crownSeed,rows=max(1,Int(rect.height/pitch)),columns=Int(rect.width/pitch)+2
        for row in 0..<rows {
            let y=rect.minY+CGFloat(row)*pitch+pitch/2
            for column in 0..<columns {
                let x=rect.minX+CGFloat(column)*pitch+(row%2==0 ? 0 : pitch/2)-2,fade=Self.fade(fading,x:x,y:y,in:rect)
                let wave=0.5+0.5*sin(Double(x)*0.05-Double(y)*0.083+seed)*(0.65+0.35*cos(Double(x)*0.013-seed*0.7))
                let radius=(0.7+3.3*CGFloat(wave))*fade
                guard radius>0.5 else{continue}
                colour.withAlphaComponent(colour.alphaComponent*(0.35+0.65*fade)).setFill()
                NSBezierPath(ovalIn:NSRect(x:x-radius,y:y-radius,width:radius*2,height:radius*2)).fill()
            }
        }
    }
    override func draw(_ dirtyRect:NSRect){
        let isDark=dark(self)
        // A soft wash lit by the colours of the tools that used the most.
        (isDark ? NSColor(srgbRed:0.05,green:0.06,blue:0.07,alpha:1) : NSColor(srgbRed:0.95,green:0.96,blue:0.97,alpha:1)).setFill();bounds.fill()
        let glows=[teal]+counted.sorted{$0.tokens>$1.tokens}.prefix(2).map{accent($0.id,self)}
        for (c,(fx,fy,fr)) in zip(glows,[(0.08,0.03,0.95),(1.0,0.22,0.8),(0.1,0.78,0.9)] as [(CGFloat,CGFloat,CGFloat)]){
            let centre=NSPoint(x:bounds.width*fx,y:bounds.height*fy)
            NSGradient(starting:c.withAlphaComponent(isDark ? 0.26 : 0.20),ending:c.withAlphaComponent(0))?.draw(fromCenter:centre,radius:0,toCenter:centre,radius:bounds.width*fr,options:[])
        }
        let dots=teal.withAlphaComponent(isDark ? 0.6 : 0.5)
        drawHalftone(in:NSRect(x:0,y:0,width:bounds.width,height:Self.topBand-8),fading:.down,colour:dots)
        drawHalftone(in:NSRect(x:0,y:bounds.height-Self.bottomBand+8,width:bounds.width,height:Self.bottomBand-8),fading:.up,colour:dots)
        let card=NSRect(x:Self.margin,y:Self.topBand,width:bounds.width-Self.margin*2,height:cardHeight),shape=NSBezierPath(roundedRect:card,xRadius:26,yRadius:26)
        NSGraphicsContext.saveGraphicsState()
        let shadow=NSShadow();shadow.shadowColor=NSColor.black.withAlphaComponent(isDark ? 0.45 : 0.09);shadow.shadowBlurRadius=36;shadow.shadowOffset=NSSize(width:0,height:-12);shadow.set()
        (isDark ? NSColor(srgbRed:0.10,green:0.105,blue:0.115,alpha:1) : .white).setFill();shape.fill()
        NSGraphicsContext.restoreGraphicsState()
        let rim=NSBezierPath(roundedRect:card.insetBy(dx:0.5,dy:0.5),xRadius:25.5,yRadius:25.5)
        NSColor.labelColor.withAlphaComponent(isDark ? 0.11 : 0.05).setStroke();rim.lineWidth=1;rim.stroke()
        let left=card.minX+Self.inset,width=card.width-Self.inset*2,right=left+width
        let quiet=NSColor.labelColor.withAlphaComponent(isDark ? 0.055 : 0.035)
        var y=card.minY+Self.inset
        // Header: what and when.
        let day=Self.period(dashboard),dayFont=font(11,.semibold),pillWidth=min(width*0.6,measure(day,dayFont)+32)
        let pill=NSRect(x:right-pillWidth,y:y,width:pillWidth,height:26)
        teal.withAlphaComponent(isDark ? 0.16 : 0.09).setFill();NSBezierPath(roundedRect:pill,xRadius:13,yRadius:13).fill()
        symbol("calendar",centre:NSPoint(x:pill.minX+15,y:pill.midY),size:10,color:teal)
        draw(day,x:pill.minX+24,middle:pill.midY,width:pillWidth-32,dayFont,teal)
        let title=Self.title(dashboard),room=pill.minX-left-10,full=measure(title,font(19,.bold))
        draw(title,x:left,middle:pill.midY,width:room+2,font(full<=room ? 19 : max(15,floor(19*room/full)),.bold),.labelColor)
        y+=44
        // Hero: the estimate, its dollar sign set small against the figure, with dots fading in from the card's
        // right edge beside it, under the figure.
        NSGraphicsContext.saveGraphicsState();shape.addClip()
        drawHalftone(in:NSRect(x:card.maxX-170,y:y-6,width:170,height:94),fading:.left,colour:teal.withAlphaComponent(isDark ? 0.34 : 0.26))
        NSGraphicsContext.restoreGraphicsState()
        let counted=self.counted,costs=counted.compactMap{$0.cost},partial=counted.contains{$0.cost==nil || $0.unpriced>0}
        draw(partial ? "已知 API 等值估算" : "API 等值估算",NSRect(x:left,y:y,width:width,height:16),font(11.5,.medium),.secondaryLabelColor)
        let hero=NSMutableAttributedString()
        if let total=costs.isEmpty ? nil : costs.reduce(0,+) {
            hero.append(NSAttributedString(string:Currency.symbol,attributes:[.font:font(26,.semibold,rounded:true),.foregroundColor:NSColor.secondaryLabelColor,.baselineOffset:15]))
            hero.append(NSAttributedString(string:Self.dollars.string(from:NSNumber(value:Currency.convert(total))) ?? String(money(total).dropFirst()),attributes:[.font:font(50,.bold,rounded:true),.foregroundColor:NSColor.labelColor,.kern:-0.6]))
        } else {hero.append(NSAttributedString(string:"—",attributes:[.font:font(50,.bold,rounded:true),.foregroundColor:NSColor.labelColor]))}
        hero.draw(in:NSRect(x:left-2,y:y+16,width:width,height:64))
        y+=88
        // Three figures, each with its own small symbol.
        let count=NumberFormatter();count.numberStyle = .decimal
        // Cache hit rate weighted by each tool's tokens; tools that report none are left out.
        let cached=counted.filter{$0.cache != nil},weight=cached.reduce(0){$0+$1.tokens}
        let hit=weight>0 ? String(format:"%.1f%%",cached.reduce(0){$0+($1.cache ?? 0)*$1.tokens}/weight*100) : "—"
        let tiles=[("square.stack.3d.up.fill","Token",tokens(counted.reduce(0){$0+$1.tokens})),
                   ("arrow.up.arrow.down","请求",count.string(from:NSNumber(value:counted.reduce(0){$0+$1.requests})) ?? "0"),
                   ("bolt.fill","缓存命中",hit)]
        let tileWidth=(width-20)/3
        for (i,(icon,label,value)) in tiles.enumerated(){
            let tile=NSRect(x:left+CGFloat(i)*(tileWidth+10),y:y,width:tileWidth,height:66)
            quiet.setFill();NSBezierPath(roundedRect:tile,xRadius:16,yRadius:16).fill()
            symbol(icon,centre:NSPoint(x:tile.minX+19,y:tile.minY+19),size:10,color:teal)
            draw(label,NSRect(x:tile.minX+30,y:tile.minY+11.5,width:tile.width-40,height:15),font(10.5,.medium),.secondaryLabelColor)
            draw(value,NSRect(x:tile.minX+13,y:tile.minY+31,width:tile.width-24,height:26),fitted(value,20,.semibold,room:tile.width-26),.labelColor)
        }
        y+=88
        // Chart: the total as a soft area, each tool as a thin line in its own colour.
        let n=dashboard.dates.count
        if n>1 {
            let series=counted.map{p in (0..<n).map{i in i<p.history.count ? max(0,p.history[i]) : 0}}
            // Same scale as the panel: a rounded ceiling with labelled half and full grid lines.
            let observed=max((0..<n).map{i in series.reduce(0){$0+$1[i]}}.max() ?? 0,1),unit=pow(10,floor(log10(observed)))
            let ceiling=ceil(observed*1.08/unit*2)/2*unit
            let plot=NSRect(x:left,y:y+16,width:width,height:Self.chartHeight-46),axisFont=font(9.5,.medium,rounded:true)
            draw(dashboard.chartUnit+" · Token",NSRect(x:right-160,y:y-2,width:160,height:13),font(9.5,.medium),.tertiaryLabelColor,.right)
            for f in [CGFloat(0),0.5,1]{
                let yy=plot.maxY-f*plot.height,grid=NSBezierPath()
                grid.move(to:NSPoint(x:plot.minX,y:yy));grid.line(to:NSPoint(x:plot.maxX,y:yy))
                if f>0 {grid.setLineDash([2,3],count:2,phase:0);grid.lineWidth=0.75;NSColor.labelColor.withAlphaComponent(isDark ? 0.12 : 0.09).setStroke()}
                else {grid.lineWidth=1;NSColor.labelColor.withAlphaComponent(isDark ? 0.16 : 0.10).setStroke()}
                grid.stroke()
            }
            let positions=DashboardView.chartPositions(dashboard)
            let paths=DashboardView.smoothCurves(series,positions:positions,plot:plot,ceiling:ceiling),total=paths.last!
            let area=total.copy() as! NSBezierPath;area.line(to:NSPoint(x:plot.maxX,y:plot.maxY));area.line(to:NSPoint(x:plot.minX,y:plot.maxY));area.close()
            NSGraphicsContext.saveGraphicsState();area.addClip()
            NSGradient(starting:teal.withAlphaComponent(isDark ? 0.34 : 0.22),ending:teal.withAlphaComponent(0.0))?.draw(from:NSPoint(x:plot.midX,y:plot.minY),to:NSPoint(x:plot.midX,y:plot.maxY),options:[])
            NSGraphicsContext.restoreGraphicsState()
            for (i,curve) in paths.enumerated(){
                let isTotal=i==paths.count-1
                curve.lineWidth=isTotal ? 2.4 : 1.3;curve.lineCapStyle = .round;curve.lineJoinStyle = .round
                (isTotal ? teal : accent(counted[i].id,self).withAlphaComponent(0.85)).setStroke();curve.stroke()
            }
            let surface=isDark ? NSColor(srgbRed:0.10,green:0.105,blue:0.115,alpha:1) : NSColor.white
            for f in [0.5,1.0]{
                let label=axisTokens(ceiling*f),yy=plot.maxY-CGFloat(f)*plot.height
                surface.withAlphaComponent(0.85).setFill();NSBezierPath(roundedRect:NSRect(x:left-3,y:yy-14,width:measure(label,axisFont)+6,height:13),xRadius:3,yRadius:3).fill()
                draw(label,NSRect(x:left,y:yy-14,width:80,height:12),axisFont,.tertiaryLabelColor)
            }
            // The latest point of the total, ringed.
            let end=total.currentPoint
            teal.withAlphaComponent(0.18).setFill();NSBezierPath(ovalIn:NSRect(x:end.x-7,y:end.y-7,width:14,height:14)).fill()
            let dot=NSBezierPath(ovalIn:NSRect(x:end.x-3.5,y:end.y-3.5,width:7,height:7))
            surface.setFill();dot.fill();teal.setStroke();dot.lineWidth=2;dot.stroke()
            let labelFont=font(9.5,.medium,rounded:true),middle=positions.indices.dropFirst().dropLast().min{abs(positions[$0]-0.5)<abs(positions[$1]-0.5)}
            for i in [0]+(middle.map{[$0]} ?? [])+[n-1]{
                let x=plot.minX+CGFloat(positions[i])*plot.width,align:NSTextAlignment=i==0 ? .left : (i==n-1 ? .right : .center)
                draw(dashboard.dates[i],NSRect(x:i==0 ? x : (i==n-1 ? x-80 : x-40),y:plot.maxY+8,width:80,height:13),labelFont,.tertiaryLabelColor,align)
            }
            y+=Self.chartHeight+22
        }
        // The usage heatmap under its heading, the unit at the right.
        if let heatmap {
            draw("用量热力图"+(heatmap.streak.map{" · "+$0} ?? ""),NSRect(x:left,y:y,width:width-90,height:15),font(11,.semibold),.secondaryLabelColor)
            draw(heatmap.hint,NSRect(x:left,y:y+1,width:width,height:14),font(10,.medium),.tertiaryLabelColor,.right)
            drawHeatmap(heatmap,x:left,y:y+24,width:width,shades:DashboardView.heatmapShades(self))
            y+=heatmapHeight
        }
        // Share of tokens: one bar in the tools' colours, then a key that names each colour.
        let sum=max(counted.reduce(0){$0+$1.tokens},1),shared=self.shared
        if !shared.isEmpty {
            draw("Token 占比",NSRect(x:left,y:y,width:width,height:15),font(11,.semibold),.secondaryLabelColor)
            let bar=NSRect(x:left,y:y+24,width:width,height:10)
            NSGraphicsContext.saveGraphicsState();NSBezierPath(roundedRect:bar,xRadius:5,yRadius:5).addClip()
            quiet.setFill();bar.fill()
            // A small share still shows as a sliver; the largest one gives up the room.
            var widths=shared.map{max(10,bar.width*CGFloat($0.tokens/sum))}
            if let widest=widths.indices.max(by:{widths[$0]<widths[$1]}){widths[widest]-=widths.reduce(0,+)-bar.width}
            var x=bar.minX
            for (i,p) in shared.enumerated(){
                accent(p.id,self).setFill();NSRect(x:x,y:bar.minY,width:max(0,widths[i]-(i<shared.count-1 ? 2 : 0)),height:bar.height).fill();x+=widths[i]
            }
            NSGraphicsContext.restoreGraphicsState()
            let column=width/3,keyFont=font(11,.medium),shareFont=font(11,.medium,rounded:true)
            for (i,p) in shared.enumerated(){
                let x=left+CGFloat(i%3)*column,ky=bar.maxY+14+CGFloat(i/3)*Self.legendRow,percent=share(p.tokens/sum)
                accent(p.id,self).setFill();NSBezierPath(ovalIn:NSRect(x:x,y:ky+4,width:7,height:7)).fill()
                let name=DashboardView.legendName(p)
                let nameWidth=min(measure(name,keyFont),column-measure(percent,shareFont)-24)
                draw(name,NSRect(x:x+12,y:ky,width:nameWidth,height:15),keyFont,.labelColor)
                draw(percent,NSRect(x:x+16+nameWidth,y:ky,width:column-nameWidth-16,height:15),shareFont,.secondaryLabelColor)
            }
            y+=60+CGFloat((shared.count+2)/3)*Self.legendRow
        }
        // Each tool on its own tile: what it cost, its share and stats, then every model.
        for p in rows {
            let c=accent(p.id,self),inSummary=p.summaryIncluded != false,block=NSRect(x:left,y:y,width:width,height:Self.blockHeight(p))
            quiet.setFill();NSBezierPath(roundedRect:block,xRadius:18,yRadius:18).fill()
            // The colour bar keeps a gutter of its own; the name and every line under it start at one edge.
            let il=block.minX+16,ir=block.maxX-16,top=block.minY+16,tl=il+12,tw=ir-tl
            c.withAlphaComponent(inSummary ? 1 : 0.45).setFill();NSBezierPath(roundedRect:NSRect(x:il,y:top+3,width:4,height:15),xRadius:2,yRadius:2).fill()
            let nameFont=font(14.5,.semibold),amount=amount(p),amountFont=font(17,.semibold,rounded:true),amountWidth=measure(amount,amountFont)
            draw(p.name,NSRect(x:tl,y:top,width:min(tw-amountWidth-12,measure(p.name,nameFont)+2),height:20),nameFont,.labelColor)
            draw(amount,NSRect(x:ir-amountWidth-2,y:top-1,width:amountWidth+2,height:22),amountFont,.labelColor,.right)
            let part=p.summaryPart?.tokens ?? 0,figures=tokenLine(p)+(part>0 ? " · 占 "+share(part/sum) : ""),figureFont=font(11.5,.medium)
            // A source a gateway partly counts says so, as the panel card does: its share is only the rest.
            // The note sits at the right of the token line, under the amount, where it has room to be read whole.
            var noteWidth:CGFloat=0
            if let tag=Self.tag(p) {
                let tagFont=font(9.5,.medium)
                noteWidth=min(tw-measure(figures,figureFont)-10,measure(tag,tagFont)+16)
                let capsule=NSRect(x:ir-noteWidth,y:top+23.5,width:noteWidth,height:17)
                NSColor.labelColor.withAlphaComponent(isDark ? 0.09 : 0.06).setFill();NSBezierPath(roundedRect:capsule,xRadius:8.5,yRadius:8.5).fill()
                draw(tag,x:capsule.minX+8,middle:capsule.midY,width:capsule.width-16,tagFont,.secondaryLabelColor,.center)
            }
            draw(figures,NSRect(x:tl,y:top+24,width:tw-noteWidth-(noteWidth>0 ? 10 : 0),height:16),figureFont,.secondaryLabelColor)
            draw(DashboardView.stats(p),NSRect(x:tl,y:top+42,width:tw,height:15),font(10.5,.regular),.secondaryLabelColor)
            var my=top+66
            if let price=Self.priceLine(p){draw(price,NSRect(x:tl,y:my-6,width:tw,height:15),font(10.5,.medium),.systemOrange);my+=18}
            for line in Self.clientLines(p,width:Self.clientWidth,font:Self.clientFont){draw(line,NSRect(x:tl,y:my-6,width:tw,height:15),Self.clientFont,.secondaryLabelColor);my+=18}
            if p.models.isEmpty{draw("此时段暂无模型调用",NSRect(x:tl,y:my,width:tw,height:15),font(11,.regular),.secondaryLabelColor)}
            for m in p.models{
                let modelFont=font(11.5,.regular),noteFont=font(10,.medium)
                var nameRoom=tw-140
                if let note=m.priceNote {
                    let width=min(nameRoom*0.55,measure(note,noteFont)),nameWidth=min(nameRoom-width-6,measure(modelName(m.name),modelFont))
                    draw(note,NSRect(x:tl+nameWidth+6,y:my+1,width:width+2,height:15),noteFont,.systemOrange);nameRoom=nameWidth
                }
                draw(modelName(m.name),NSRect(x:tl,y:my,width:nameRoom+2,height:16),modelFont,.labelColor)
                draw(modelAmount(m),NSRect(x:ir-134,y:my,width:86,height:16),font(11.5,.medium,rounded:true),.labelColor,.right)
                draw(share(m.share),NSRect(x:ir-44,y:my,width:44,height:16),font(11,.medium,rounded:true),.secondaryLabelColor,.right)
                let track=NSRect(x:tl,y:my+17,width:tw,height:3)
                NSColor.labelColor.withAlphaComponent(isDark ? 0.08 : 0.06).setFill();NSBezierPath(roundedRect:track,xRadius:1.5,yRadius:1.5).fill()
                let fill=NSRect(x:track.minX,y:track.minY,width:max(3,track.width*CGFloat(min(1,m.share))),height:track.height)
                NSGradient(starting:c.withAlphaComponent(0.55),ending:c)?.draw(in:NSBezierPath(roundedRect:fill,xRadius:1.5,yRadius:1.5),angle:0)
                my+=Self.modelHeight
            }
            y+=block.height+10
        }
        // Signature under the card: the mark, the name and where to find it.
        let footer=card.maxY+20,nameFont=font(13.5,.semibold),nameWidth=measure(project.name,nameFont),rowWidth=20+8+nameWidth
        brandMark(NSRect(x:bounds.midX-rowWidth/2,y:footer,width:20,height:20))
        draw(project.name,NSRect(x:bounds.midX-rowWidth/2+28,y:footer+1,width:nameWidth+2,height:18),nameFont,.labelColor)
        draw(project.url,NSRect(x:0,y:footer+26,width:bounds.width,height:15),font(10.5,.regular),.secondaryLabelColor,.center)
    }
}
// The terminal style: a terminal window on a dot grid.
final class TerminalShareCard:ShareCard {
    static let margin:CGFloat=22,pad:CGFloat=22,titleBar:CGFloat=34,chartHeight:CGFloat=170,modelHeight:CGFloat=20,legendRow:CGFloat=18
    static let clientWidth=width-margin*2-pad*2-16,clientFont=NSFont.monospacedSystemFont(ofSize:10.5,weight:.regular)
    static func blockHeight(_ p:ProviderUsage)->CGFloat {90+(priceLine(p)==nil ? 0 : 17)+CGFloat(clientLines(p,width:clientWidth,font:clientFont).count)*17+CGFloat(max(1,p.models.count))*modelHeight}
    private var windowHeight:CGFloat {
        let chart=dashboard.dates.count>1 ? Self.chartHeight : 0
        let share:CGFloat=shared.isEmpty ? 0 : 56+CGFloat((shared.count+2)/3)*Self.legendRow
        return Self.titleBar+Self.pad*2+32+46+80+80+chart+heatmapHeight+share+rows.reduce(0){$0+Self.blockHeight($1)}+26
    }
    private static let heatmapWidth=width-margin*2-pad*2
    private var heatmapHeight:CGFloat {heatmap.map{24+Self.heatmapGridHeight($0,width:Self.heatmapWidth)+24} ?? 0}
    override var pictureHeight:CGFloat {Self.topBand+windowHeight+58+Self.bottomBand}
    // What the picture covers, written as dates: today, a 24-hour window, or first to last day.
    static func span(_ d:Dashboard)->String {
        func numbers(_ value:String)->[Int] {value.split(whereSeparator:{"-T:".contains($0)}).compactMap{Int($0)}}
        guard d.rangeKey != "today",let bounds=d.period,bounds.count==2 else{return d.date}
        let a=numbers(bounds[0]),b=numbers(bounds[1])
        guard a.count>=3,b.count==a.count else{return d.date}
        if a.count>=5 {return String(format:"%02d-%02d %02d:%02d → %02d-%02d %02d:%02d",a[1],a[2],a[3],a[4],b[1],b[2],b[3],b[4])}
        if a[0] != b[0] {return String(format:"%d-%02d-%02d → %d-%02d-%02d",a[0],a[1],a[2],b[0],b[1],b[2])}
        return a[1]==b[1] && a[2]==b[2] ? String(format:"%d-%02d-%02d",a[0],a[1],a[2]) : String(format:"%d-%02d-%02d → %02d-%02d",a[0],a[1],a[2],b[1],b[2])
    }
    // A terminal's colours, light or dark with the appearance.
    private struct Palette {let base,dots,window,bar,text,muted,comment,accent,rule:NSColor}
    private static func rgb(_ r:CGFloat,_ g:CGFloat,_ b:CGFloat,_ a:CGFloat=1)->NSColor {NSColor(srgbRed:r,green:g,blue:b,alpha:a)}
    private var palette:Palette {
        dark(self) ? Palette(base:Self.rgb(0.04,0.055,0.07),dots:Self.rgb(1,1,1,0.07),window:Self.rgb(0.06,0.08,0.10),bar:Self.rgb(0.09,0.11,0.14),
                             text:Self.rgb(0.90,0.93,0.95),muted:Self.rgb(0.62,0.66,0.70),comment:Self.rgb(0.45,0.50,0.55),accent:Self.rgb(0.25,0.85,0.72),rule:Self.rgb(1,1,1,0.10))
                   : Palette(base:Self.rgb(0.95,0.96,0.97),dots:Self.rgb(0,0,0,0.10),window:.white,bar:Self.rgb(0.95,0.96,0.97),
                             text:Self.rgb(0.11,0.13,0.15),muted:Self.rgb(0.36,0.40,0.44),comment:Self.rgb(0.52,0.56,0.60),accent:Self.rgb(0.02,0.54,0.46),rule:Self.rgb(0,0,0,0.09))
    }
    private func mono(_ size:CGFloat,_ weight:NSFont.Weight = .regular)->NSFont {NSFont.monospacedSystemFont(ofSize:size,weight:weight)}
    // The terminal's pattern: columns of hex digits raining down, each led by a bright head with a fading trail.
    // Columns are keyed by their place in the picture, so every patch of rain belongs to the same field.
    private func drawRain(in rect:NSRect,fading:Fade,_ c:Palette,brightness:CGFloat=1){
        let pitch:CGFloat=10,seed=crownSeed,rows=max(1,Int(rect.height/pitch)),columns=Int(rect.width/pitch)+1
        let font=mono(8.5,.medium),digits=Array("0123456789abcdef")
        for column in 0..<columns {
            let x=rect.minX+CGFloat(column)*pitch,key=Int((x/pitch).rounded()),head=Int(Self.grain(key,7,seed)%UInt32(rows+4))
            for row in 0..<rows {
                let y=rect.minY+CGFloat(row)*pitch,grain=Self.grain(key,row+Int(rect.minY/pitch),seed),behind=head-row
                let fade=Self.fade(fading,x:x+pitch/2,y:y+pitch/2,in:rect)
                // The head is brightest; the trail above it dims; elsewhere a faint scatter of digits.
                let strength:CGFloat=behind==0 ? 1 : (behind>0 && behind<5 ? 0.62-CGFloat(behind)*0.11 : (grain%3==0 ? 0.14 : 0))
                guard strength>0,fade>0.02 else{continue}
                let colour=(behind==0 ? c.text : c.accent).withAlphaComponent(strength*fade*brightness)
                draw(String(digits[Int(grain%16)]),NSRect(x:x+1,y:y,width:pitch,height:pitch+2),font,colour,.center)
            }
        }
    }
    private func ui(_ size:CGFloat,_ weight:NSFont.Weight)->NSFont {NSFont.systemFont(ofSize:size,weight:weight)}
    // Large figures shrink to fit their slot rather than truncate.
    private func fitted(_ value:String,_ size:CGFloat,room:CGFloat)->NSFont {
        let measured=measure(value,mono(size,.semibold))
        return measured<=room ? mono(size,.semibold) : mono(max(11,floor(size*room/measured)),.semibold)
    }
    // Shares as a row of cells: each tool gets cells in proportion, at least one if it used anything.
    private static func cells(_ shares:[Double],count:Int)->[Int] {
        var result=shares.map{max($0>0 ? 1 : 0,Int(($0*Double(count)).rounded(.down)))}
        let order=shares.indices.sorted{shares[$0]*Double(count)-floor(shares[$0]*Double(count))>shares[$1]*Double(count)-floor(shares[$1]*Double(count))}
        var i=0
        while result.reduce(0,+)<count && !order.isEmpty {result[order[i%order.count]]+=1;i+=1}
        while result.reduce(0,+)>count,let widest=result.indices.max(by:{result[$0]<result[$1]}) {result[widest]-=1}
        return result
    }
    private func dashedRule(_ y:CGFloat,_ left:CGFloat,_ right:CGFloat,_ color:NSColor){
        let path=NSBezierPath();path.move(to:NSPoint(x:left,y:y));path.line(to:NSPoint(x:right,y:y))
        path.setLineDash([3,3],count:2,phase:0);path.lineWidth=1;color.setStroke();path.stroke()
    }
    private func prompt(_ command:String,x:CGFloat,middle:CGFloat,cursor:Bool,_ c:Palette){
        draw("❯",x:x,middle:middle,width:14,mono(12.5,.bold),c.accent)
        draw(command,x:x+16,middle:middle,width:Self.width,mono(12,.medium),c.text)
        if cursor {
            let after=x+16+measure(command,mono(12,.medium))+(command.isEmpty ? 0 : 6)
            c.accent.withAlphaComponent(0.85).setFill();NSRect(x:after,y:middle-7,width:7.5,height:14).fill()
        }
    }
    override func draw(_ dirtyRect:NSRect){
        let isDark=dark(self),c=palette
        // A dot grid, lit faintly behind the window.
        c.base.setFill();bounds.fill()
        c.dots.setFill()
        var gy:CGFloat=7
        while gy<bounds.height {var gx:CGFloat=7;while gx<bounds.width {NSRect(x:gx-0.6,y:gy-0.6,width:1.2,height:1.2).fill();gx+=14};gy+=14}
        let glow=NSPoint(x:bounds.width*0.15,y:60)
        NSGradient(starting:c.accent.withAlphaComponent(isDark ? 0.16 : 0.12),ending:c.accent.withAlphaComponent(0))?.draw(fromCenter:glow,radius:0,toCenter:glow,radius:bounds.width*0.9,options:[])
        // The window: title bar with its three lights, then the session.
        drawRain(in:NSRect(x:0,y:0,width:bounds.width,height:Self.topBand-8),fading:.down,c)
        drawRain(in:NSRect(x:0,y:bounds.height-Self.bottomBand+8,width:bounds.width,height:Self.bottomBand-8),fading:.up,c)
        let window=NSRect(x:Self.margin,y:Self.topBand,width:bounds.width-Self.margin*2,height:windowHeight),shape=NSBezierPath(roundedRect:window,xRadius:12,yRadius:12)
        NSGraphicsContext.saveGraphicsState()
        let shadow=NSShadow();shadow.shadowColor=NSColor.black.withAlphaComponent(isDark ? 0.55 : 0.14);shadow.shadowBlurRadius=34;shadow.shadowOffset=NSSize(width:0,height:-14);shadow.set()
        c.window.setFill();shape.fill()
        NSGraphicsContext.restoreGraphicsState()
        NSGraphicsContext.saveGraphicsState();shape.addClip()
        c.bar.setFill();NSRect(x:window.minX,y:window.minY,width:window.width,height:Self.titleBar).fill()
        NSGraphicsContext.restoreGraphicsState()
        line(NSPoint(x:window.minX,y:window.minY+Self.titleBar),NSPoint(x:window.maxX,y:window.minY+Self.titleBar),color:c.rule,width:1)
        let rim=NSBezierPath(roundedRect:window.insetBy(dx:0.5,dy:0.5),xRadius:11.5,yRadius:11.5);c.rule.setStroke();rim.lineWidth=1;rim.stroke()
        for (i,light) in [Self.rgb(1,0.37,0.34),Self.rgb(1,0.74,0.18),Self.rgb(0.16,0.78,0.25)].enumerated(){
            light.setFill();NSBezierPath(ovalIn:NSRect(x:window.minX+14+CGFloat(i)*18,y:window.minY+Self.titleBar/2-6,width:12,height:12)).fill()
        }
        draw("starmeter — usage",x:window.minX,middle:window.minY+Self.titleBar/2,width:window.width,mono(11,.medium),c.muted,.center)
        let left=window.minX+Self.pad,width=window.width-Self.pad*2,right=left+width
        var y=window.minY+Self.titleBar+Self.pad
        let flag=["today":"today","24h":"24h","7d":"7d","30d":"30d","week":"week","month":"month","all":"all"][dashboard.rangeKey] ?? dashboard.rangeKey
        prompt("starmeter usage --range "+flag,x:left,middle:y+9,cursor:false,c)
        y+=32
        // Title, and the dates it covers in a bracket-like tag.
        let span=Self.span(dashboard),spanFont=mono(10.5,.semibold),tag=NSRect(x:right-min(width*0.62,measure(span,spanFont)+18),y:y,width:min(width*0.62,measure(span,spanFont)+18),height:24)
        c.accent.withAlphaComponent(isDark ? 0.12 : 0.08).setFill();NSBezierPath(roundedRect:tag,xRadius:5,yRadius:5).fill()
        let tagRim=NSBezierPath(roundedRect:tag.insetBy(dx:0.5,dy:0.5),xRadius:4.5,yRadius:4.5);c.accent.withAlphaComponent(0.4).setStroke();tagRim.lineWidth=1;tagRim.stroke()
        draw(span,x:tag.minX+9,middle:tag.midY,width:tag.width-14,spanFont,c.accent)
        let title=Self.title(dashboard),room=tag.minX-left-10,full=measure(title,ui(19,.bold))
        draw(title,x:left,middle:tag.midY,width:room+2,ui(full<=room ? 19 : max(15,floor(19*room/full)),.bold),c.text)
        y+=46
        // The estimate, set like output, with the cursor after it, and rain fading in from the window's right
        // edge beside it, under the figure.
        NSGraphicsContext.saveGraphicsState();shape.addClip()
        drawRain(in:NSRect(x:window.maxX-140,y:y-4,width:140,height:84),fading:.left,c,brightness:0.55)
        NSGraphicsContext.restoreGraphicsState()
        let counted=self.counted,costs=counted.compactMap{$0.cost},partial=counted.contains{$0.cost==nil || $0.unpriced>0}
        draw(partial ? "// 已知 API 等值估算" : "// API 等值估算",NSRect(x:left,y:y,width:width,height:15),mono(11),c.comment)
        let total=costs.isEmpty ? "—" : Currency.symbol+(Self.dollars.string(from:NSNumber(value:Currency.convert(costs.reduce(0,+)))) ?? "0.00")
        let heroFont=mono(min(42,floor(42*width*0.86/max(1,measure(total,mono(42,.bold))))),.bold),heroTop=y+18
        let hero=NSMutableAttributedString(string:total,attributes:[.font:heroFont,.foregroundColor:c.text,.kern:-1])
        if total.hasPrefix(Currency.symbol){hero.addAttribute(.foregroundColor,value:c.accent,range:NSRange(location:0,length:1))}
        hero.draw(in:NSRect(x:left-1,y:heroTop,width:width,height:52))
        let heroWidth=hero.size().width
        c.accent.withAlphaComponent(0.85).setFill()
        NSRect(x:left+heroWidth+4,y:heroTop+heroFont.ascender-heroFont.capHeight-1,width:heroFont.pointSize*0.32,height:heroFont.capHeight+3).fill()
        y+=80
        // Three figures in a ruled table.
        let count=NumberFormatter();count.numberStyle = .decimal
        // Cache hit rate weighted by each tool's tokens; tools that report none are left out.
        let cached=counted.filter{$0.cache != nil},weight=cached.reduce(0){$0+$1.tokens}
        let hit=weight>0 ? String(format:"%.1f%%",cached.reduce(0){$0+($1.cache ?? 0)*$1.tokens}/weight*100) : "—"
        let figures=[("Token",tokens(counted.reduce(0){$0+$1.tokens})),("请求",count.string(from:NSNumber(value:counted.reduce(0){$0+$1.requests})) ?? "0"),("缓存命中",hit)]
        let table=NSRect(x:left,y:y,width:width,height:58),column=width/3
        c.bar.setFill();NSBezierPath(roundedRect:table,xRadius:7,yRadius:7).fill()
        let tableRim=NSBezierPath(roundedRect:table.insetBy(dx:0.5,dy:0.5),xRadius:6.5,yRadius:6.5);c.rule.setStroke();tableRim.lineWidth=1;tableRim.stroke()
        for (i,(label,value)) in figures.enumerated(){
            let x=table.minX+CGFloat(i)*column
            if i>0{line(NSPoint(x:x,y:table.minY),NSPoint(x:x,y:table.maxY),color:c.rule,width:1)}
            draw("▸ "+label,NSRect(x:x+12,y:table.minY+10,width:column-18,height:14),mono(10,.medium),c.muted)
            draw(value,NSRect(x:x+12,y:table.minY+28,width:column-18,height:24),fitted(value,18,room:column-22),c.text)
        }
        y+=80
        // Trend on an oscilloscope grid: the total glows, each tool is a thin line in its own colour.
        let n=dashboard.dates.count
        if n>1 {
            draw("// 用量趋势 · "+dashboard.chartUnit+" Token",NSRect(x:left,y:y,width:width,height:15),mono(10.5),c.comment)
            let series=counted.map{p in (0..<n).map{i in i<p.history.count ? max(0,p.history[i]) : 0}}
            // Same scale as the panel: a rounded ceiling with labelled half and full grid lines.
            let observed=max((0..<n).map{i in series.reduce(0){$0+$1[i]}}.max() ?? 0,1),unit=pow(10,floor(log10(observed)))
            let ceiling=ceil(observed*1.08/unit*2)/2*unit
            let plot=NSRect(x:left,y:y+34,width:width,height:104),axisFont=mono(9)
            for f in stride(from:CGFloat(0),through:1,by:0.2){
                let x=plot.minX+f*plot.width,grid=NSBezierPath();grid.move(to:NSPoint(x:x,y:plot.minY));grid.line(to:NSPoint(x:x,y:plot.maxY))
                grid.setLineDash([1,3],count:2,phase:0);grid.lineWidth=0.75;c.rule.setStroke();grid.stroke()
            }
            for f in [CGFloat(0),0.5,1]{
                let yy=plot.maxY-f*plot.height,grid=NSBezierPath();grid.move(to:NSPoint(x:plot.minX,y:yy));grid.line(to:NSPoint(x:plot.maxX,y:yy))
                if f>0 {grid.setLineDash([1,3],count:2,phase:0)};grid.lineWidth=f>0 ? 0.75 : 1;(f>0 ? c.rule : c.muted.withAlphaComponent(0.35)).setStroke();grid.stroke()
            }
            let positions=DashboardView.chartPositions(dashboard)
            let paths=DashboardView.smoothCurves(series,positions:positions,plot:plot,ceiling:ceiling),total=paths.last!
            let area=total.copy() as! NSBezierPath;area.line(to:NSPoint(x:plot.maxX,y:plot.maxY));area.line(to:NSPoint(x:plot.minX,y:plot.maxY));area.close()
            NSGraphicsContext.saveGraphicsState();area.addClip()
            NSGradient(starting:c.accent.withAlphaComponent(isDark ? 0.28 : 0.18),ending:c.accent.withAlphaComponent(0.0))?.draw(from:NSPoint(x:plot.midX,y:plot.minY),to:NSPoint(x:plot.midX,y:plot.maxY),options:[])
            NSGraphicsContext.restoreGraphicsState()
            for (i,curve) in paths.enumerated(){
                let isTotal=i==paths.count-1,colour=isTotal ? c.accent : accent(counted[i].id,self).withAlphaComponent(0.9)
                NSGraphicsContext.saveGraphicsState()
                let glowShadow=NSShadow();glowShadow.shadowColor=colour.withAlphaComponent(isTotal ? (isDark ? 0.8 : 0.45) : (isDark ? 0.5 : 0.0));glowShadow.shadowBlurRadius=isTotal ? 8 : 4;glowShadow.shadowOffset = .zero;glowShadow.set()
                curve.lineWidth=isTotal ? 2.2 : 1.2;curve.lineCapStyle = .round;curve.lineJoinStyle = .round;colour.setStroke();curve.stroke()
                NSGraphicsContext.restoreGraphicsState()
            }
            for f in [0.5,1.0]{
                let label=axisTokens(ceiling*f),yy=plot.maxY-CGFloat(f)*plot.height
                c.window.withAlphaComponent(0.85).setFill();NSRect(x:left,y:yy-13,width:measure(label,axisFont)+5,height:12).fill()
                draw(label,NSRect(x:left+1,y:yy-13,width:80,height:12),axisFont,c.comment)
            }
            let end=total.currentPoint
            c.accent.withAlphaComponent(0.22).setFill();NSBezierPath(ovalIn:NSRect(x:end.x-7,y:end.y-7,width:14,height:14)).fill()
            c.accent.setFill();NSBezierPath(ovalIn:NSRect(x:end.x-3,y:end.y-3,width:6,height:6)).fill()
            let middle=positions.indices.dropFirst().dropLast().min{abs(positions[$0]-0.5)<abs(positions[$1]-0.5)}
            for i in [0]+(middle.map{[$0]} ?? [])+[n-1]{
                let x=plot.minX+CGFloat(positions[i])*plot.width,align:NSTextAlignment=i==0 ? .left : (i==n-1 ? .right : .center)
                draw(dashboard.dates[i],NSRect(x:i==0 ? x : (i==n-1 ? x-80 : x-40),y:plot.maxY+7,width:80,height:13),mono(9.5),c.comment,align)
            }
            y+=Self.chartHeight
        }
        // The usage heatmap under its comment, the unit at the right.
        if let heatmap {
            draw("// 用量热力图"+(heatmap.streak.map{" · "+$0} ?? ""),NSRect(x:left,y:y,width:width-90,height:15),mono(10.5),c.comment)
            draw(heatmap.hint,NSRect(x:left,y:y+1,width:width,height:14),mono(9.5),c.comment,.right)
            drawHeatmap(heatmap,x:left,y:y+24,width:width,shades:[c.rule]+[0.3,0.52,0.76,1].map{c.accent.withAlphaComponent($0)})
            y+=heatmapHeight
        }
        // Share of tokens as a row of cells, then a key that names each colour.
        let sum=max(counted.reduce(0){$0+$1.tokens},1),shared=self.shared
        if !shared.isEmpty {
            draw("// Token 占比",NSRect(x:left,y:y,width:width,height:15),mono(10.5),c.comment)
            let total=40,gap:CGFloat=2,cell=(width-gap*CGFloat(total-1))/CGFloat(total)
            var index=0
            for (p,n) in zip(shared,Self.cells(shared.map{$0.tokens/sum},count:total)){
                for _ in 0..<n {accent(p.id,self).setFill();NSBezierPath(roundedRect:NSRect(x:left+CGFloat(index)*(cell+gap),y:y+26,width:cell,height:10),xRadius:1.5,yRadius:1.5).fill();index+=1}
            }
            let column=width/3,keyFont=ui(11,.medium),shareFont=mono(10.5,.medium)
            for (i,p) in shared.enumerated(){
                let x=left+CGFloat(i%3)*column,ky=y+48+CGFloat(i/3)*Self.legendRow,percent=share(p.tokens/sum)
                accent(p.id,self).setFill();NSBezierPath(roundedRect:NSRect(x:x,y:ky+3.5,width:8,height:8),xRadius:2,yRadius:2).fill()
                let name=DashboardView.legendName(p),nameWidth=min(measure(name,keyFont),column-measure(percent,shareFont)-24)
                draw(name,x:x+13,middle:ky+7.5,width:nameWidth,keyFont,c.text)
                draw(percent,x:x+18+nameWidth,middle:ky+7.5,width:column-nameWidth-18,shareFont,c.muted)
            }
            y+=56+CGFloat((shared.count+2)/3)*Self.legendRow
        }
        // Each tool as a listing: cost, share and stats, then its models as a tree with cell bars.
        for p in rows {
            let colour=accent(p.id,self),inSummary=p.summaryIncluded != false,top=y
            dashedRule(top,left,right,c.rule)
            colour.withAlphaComponent(inSummary ? 1 : 0.4).setFill();NSBezierPath(ovalIn:NSRect(x:left,y:top+20,width:8,height:8)).fill()
            let nameFont=ui(14.5,.semibold),cost=amount(p),costFont=mono(14.5,.semibold),costWidth=measure(cost,costFont)
            let nameWidth=min(width*0.45,measure(p.name,nameFont))
            draw(p.name,x:left+16,middle:top+24,width:min(nameWidth+2,width-16-costWidth-12),nameFont,c.text)
            draw(cost,x:right-costWidth-2,middle:top+24,width:costWidth+2,costFont,c.text,.right)
            let part=p.summaryPart?.tokens ?? 0,figures=tokenLine(p)+(part>0 ? " · 占 "+share(part/sum) : ""),lineFont=mono(10.5,.medium)
            // A source a gateway partly counts says so, as the panel card does: its share is only the rest.
            // The note sits at the right of the token line, under the cost, where it has room to be read whole.
            var noteWidth:CGFloat=0
            let noteFont=mono(9.5,.medium)
            if let note=Self.tag(p) {
                noteWidth=min(width-16-measure(figures,lineFont)-10,measure(note,noteFont)+14)
                let box=NSRect(x:right-noteWidth,y:top+39.5,width:noteWidth,height:16)
                let boxPath=NSBezierPath(roundedRect:box.insetBy(dx:0.5,dy:0.5),xRadius:3.5,yRadius:3.5);c.muted.withAlphaComponent(0.45).setStroke();boxPath.lineWidth=1;boxPath.stroke()
                draw(note,x:box.minX+7,middle:box.midY,width:box.width-14,noteFont,c.muted,.center)
            }
            draw(figures,NSRect(x:left+16,y:top+40,width:width-16-noteWidth-(noteWidth>0 ? 10 : 0),height:15),lineFont,c.muted)
            draw(DashboardView.stats(p),NSRect(x:left+16,y:top+57,width:width-16,height:15),mono(10.5),c.comment)
            var my=top+80
            if let price=Self.priceLine(p){draw("! "+price,NSRect(x:left+16,y:my-6,width:width-16,height:15),mono(10.5,.medium),.systemOrange);my+=17}
            for line in Self.clientLines(p,width:Self.clientWidth,font:Self.clientFont){draw(line,NSRect(x:left+16,y:my-6,width:width-16,height:15),Self.clientFont,c.comment);my+=17}
            if p.models.isEmpty{draw("此时段暂无模型调用",NSRect(x:left+16,y:my,width:width-16,height:15),mono(11),c.comment)}
            // Models hang off a drawn tree: one stem, a tick per model.
            if !p.models.isEmpty {
                let stem=left+20,branch=c.comment.withAlphaComponent(0.7)
                line(NSPoint(x:stem,y:my+1),NSPoint(x:stem,y:my+8+CGFloat(p.models.count-1)*Self.modelHeight),color:branch,width:1)
                for i in p.models.indices {let mid=my+8+CGFloat(i)*Self.modelHeight;line(NSPoint(x:stem,y:mid),NSPoint(x:stem+10,y:mid),color:branch,width:1)}
            }
            // Columns sized to this tool's longest model name and amount; the cells take what is left.
            let modelFont=mono(11),figureFont=mono(11,.medium)
            let noted={(m:ModelUsage)->CGFloat in self.measure(modelName(m.name),modelFont)+(m.priceNote.map{self.measure($0,noteFont)+6} ?? 0)}
            let nameColumn=min(170,(p.models.map(noted).max() ?? 0)+12)
            let amountColumn=max(50,(p.models.map{measure(modelAmount($0),figureFont)}.max() ?? 0)+4)
            let barLeft=left+40+nameColumn,barRoom=right-44-amountColumn-10-barLeft,slots=max(4,min(16,Int(barRoom/6.5)))
            for m in p.models{
                var nameRoom=nameColumn-8
                if let note=m.priceNote {
                    let width=min(nameRoom*0.55,measure(note,noteFont)),nameWidth=min(nameRoom-width-6,measure(modelName(m.name),modelFont))
                    draw(note,x:left+38+nameWidth+6,middle:my+8,width:width+2,noteFont,NSColor.systemOrange);nameRoom=nameWidth
                }
                draw(modelName(m.name),x:left+38,middle:my+8,width:nameRoom+1,modelFont,c.text)
                let lit=m.share>0 ? max(1,Int((min(1,m.share)*Double(slots)).rounded())) : 0
                for k in 0..<slots {
                    (k<lit ? colour.withAlphaComponent(inSummary ? 0.95 : 0.5) : c.rule).setFill()
                    NSBezierPath(roundedRect:NSRect(x:barLeft+CGFloat(k)*6.5,y:my+4,width:5,height:8),xRadius:1,yRadius:1).fill()
                }
                draw(modelAmount(m),x:right-44-amountColumn,middle:my+8,width:amountColumn,figureFont,c.text,.right)
                draw(share(m.share),x:right-40,middle:my+8,width:40,mono(10.5),c.muted,.right)
                my+=Self.modelHeight
            }
            y+=Self.blockHeight(p)
        }
        // The session waits at a prompt.
        dashedRule(y,left,right,c.rule)
        prompt("",x:left,middle:y+16,cursor:true,c)
        // Signature under the window: the mark, the name and where to find it.
        let footer=window.maxY+18,signFont=ui(13.5,.semibold),signWidth=measure(project.name,signFont),rowWidth=20+8+signWidth
        brandMark(NSRect(x:bounds.midX-rowWidth/2,y:footer,width:20,height:20))
        draw(project.name,x:bounds.midX-rowWidth/2+28,middle:footer+10,width:signWidth+2,signFont,c.text)
        draw(project.url,NSRect(x:0,y:footer+26,width:bounds.width,height:14),mono(10),c.comment,.center)
    }
}
final class DashboardView:FlippedView {
    private(set) var data:Dashboard;var toggleProvider:((String,Bool)->Void)?;private var hoveredProvider:String?;private var heldProvider:String?
    private var hoveredDay:Int?;private var heatmapTracking:NSTrackingArea?
    // Worked out once: measuring the shared column means scaling the chart, far too slow to do per square.
    private let heatmapGeometry:HeatmapLayout?,heatmapColumn:CGFloat,heatmapLevels:[Int]
    private var summaryProviders:[ProviderUsage] {data.providers.compactMap{$0.summaryPart}}
    // Pinned: the live chart takes the trend's place, the same chart kept from view to view so it never restarts.
    var live:LiveChart? {
        didSet {
            guard let chart=live,data.showSummary != false,!data.dates.isEmpty else{return}
            let first=chart.superview==nil
            chart.frame=NSRect(x:12,y:Self.chartExtra(data)+104,width:bounds.width-24,height:122);chart.column=Self.labelColumn(data)
            chart.total=Self.totalColor(self);chart.order=summaryProviders.filter{$0.available}.map{$0.id}
            chart.colors=Dictionary(uniqueKeysWithValues:chart.order.map{($0,accent($0,self))})
            addSubview(chart);needsDisplay=true
            if first && !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
                chart.alphaValue=0;NSAnimationContext.runAnimationGroup{context in context.duration=0.35;chart.animator().alphaValue=1}
            }
        }
    }
    // The space under a card's last model row, before the next card's name.
    static let cardGap:CGFloat=8
    // A source with nothing to show yet says so once, under its name, with no empty model list below.
    static func sectionHeight(_ p:ProviderUsage)->CGFloat {p.available ? 52+clientHeight(p)+CGFloat(max(1,p.models.count))*18+cardGap : 44+cardGap}
    // A gateway's clients in one line, by their share of its tokens: "客户端 · ChatGPT 62% · Claude Code 30%".
    static func clientLine(_ p:ProviderUsage)->String? {
        guard p.available,let clients=p.clients,!clients.isEmpty else{return nil}
        return "客户端 · "+clients.map{$0.name+" "+share($0.share)}.joined(separator:" · ")
    }
    static func clientHeight(_ p:ProviderUsage)->CGFloat {clientLine(p)==nil ? 0 : 16}
    static func providerHeight(_ p:ProviderUsage,_ d:Dashboard)->CGFloat {
        guard d.collapsible else{return sectionHeight(p)}
        return (d.expandedProviders ?? []).contains(p.id) ? sectionHeight(p) : 52
    }
    // Legend names drop suffixes the cards below already show, so four fit on a row.
    static func legendLabels(_ d:Dashboard)->[String] {["总量"]+d.providers.compactMap{$0.summaryPart}.map(legendName)}
    static func legendName(_ p:ProviderUsage)->String {
        p.id=="claude" ? "Claude" : p.name.hasSuffix(" Harness") ? String(p.name.dropLast(" Harness".count)) : p.name
    }
    // The dashboard is always 344pt wide, so the legend gets 320pt between the margins.
    static func chartExtra(_ d:Dashboard)->CGFloat {CGFloat(max(0,legendLayout(legendLabels(d),width:320).count-1))*17}
    static func summaryHeight(_ d:Dashboard)->CGFloat {d.showSummary==false ? 44 : 230+chartExtra(d)}
    // The heatmap's hint line already keeps the first card clear, so no gap is added after it.
    static func harnessHeaderHeight(_ d:Dashboard)->CGFloat {d.providers.isEmpty || d.showSummary==false || d.heatmap != nil ? 0 : 6}
    // The choice of grouping sits above the cards, when there are cards and more than one way to group them.
    static func groupingHeight(_ d:Dashboard)->CGFloat {d.providers.isEmpty || (d.groupings?.count ?? 0)<2 ? 0 : 30}
    static func cardsTop(_ d:Dashboard)->CGFloat {summaryHeight(d)+heatmapHeight(d)+harnessHeaderHeight(d)+groupingHeight(d)}
    static func height(_ d:Dashboard)->CGFloat {cardsTop(d)+max(d.cardList.isEmpty && !d.providers.isEmpty ? 24 : 0,d.cardList.reduce(0){$0+providerHeight($1,d)})+(d.providers.isEmpty ? 0 : 4)}
    static func totalColor(_ view:NSView)->NSColor {dark(view) ? NSColor(srgbRed:0.43,green:0.79,blue:0.75,alpha:1) : NSColor(srgbRed:0.15,green:0.47,blue:0.46,alpha:1)}
    // The chart's scale: a rounded ceiling over the highest total, labelled at zero, half and full.
    static func chartAxis(_ d:Dashboard)->(ceiling:Double,labels:[String]) {
        let n=d.dates.count,series=d.providers.compactMap{$0.summaryPart}.map{p in (0..<n).map{i in i<p.history.count ? max(0,p.history[i]) : 0}}
        let observed=max((0..<n).map{i in series.reduce(0){$0+$1[i]}}.max() ?? 0,1),unit=pow(10,floor(log10(observed)))
        let ceiling=ceil(observed*1.08/unit*2)/2*unit
        return (ceiling,[0,0.5,1].map{axisTokens(ceiling*$0)})
    }
    static let axisFont=NSFont.monospacedDigitSystemFont(ofSize:8,weight:.regular),rowFont=NSFont.systemFont(ofSize:8)
    // One column on the left holds the chart's axis labels and the heatmap's row names, so the plot above and
    // the squares below start at the same place and both run to the right edge, like a single chart.
    static func labelColumn(_ d:Dashboard)->CGFloat {
        func widest(_ values:[String],_ font:NSFont)->CGFloat {values.map{ceil(($0 as NSString).size(withAttributes:[.font:font]).width)}.max() ?? 0}
        let axis=d.showSummary==false || d.dates.isEmpty ? 0 : widest(chartAxis(d).labels,axisFont)
        return max(axis,widest(d.heatmap?.rowLabels ?? [],rowFont))+10
    }
    // The heatmap sits under Summary in the room after the shared column, with its unit in small type at the bottom right.
    static func heatmapLayout(_ d:Dashboard)->HeatmapLayout? {
        guard let h=d.heatmap else{return nil}
        let column=labelColumn(d)
        return HeatmapLayout(origin:NSPoint(x:12+column,y:summaryHeight(d)+8),width:344-24-column,columns:h.columns,rows:h.rows)
    }
    static func heatmapHeight(_ d:Dashboard)->CGFloat {heatmapLayout(d).map{8+$0.frame.height+15} ?? 0}
    static func heatmapGrid(_ d:Dashboard,width:CGFloat)->NSRect {heatmapLayout(d)?.frame ?? .zero}
    static func heatmapCell(_ d:Dashboard,width:CGFloat,index:Int)->NSRect {heatmapLayout(d)?.square(index) ?? .zero}
    // The square under a point, or nil outside the grid and for squares still to come.
    static func heatmapIndex(at point:NSPoint,_ d:Dashboard,width:CGFloat)->Int? {
        guard let h=d.heatmap,let layout=heatmapLayout(d) else{return nil}
        return heatmapIndex(at:point,h,layout)
    }
    static func heatmapIndex(at point:NSPoint,_ h:UsageHeatmap,_ layout:HeatmapLayout)->Int? {
        layout.index(at:point).flatMap{$0<h.past ? $0 : nil}
    }
    // The line under the squares: the square pointed at (or why there's nothing yet) on the left, its length on the right.
    static func heatmapCaption(_ h:UsageHeatmap,pointed:Int?)->String? {pointed.map{h.detail($0)} ?? h.note}
    static func heading(_ d:Dashboard)->String {d.showSummary==false ? "Harness 用量" : "Summary"}
    // The heading ends clear of the buttons beside it; the leftmost one (自动检测) starts 209pt from the right.
    // The heading ends 6pt before the leftmost header button (the pin, 233pt from the right).
    static func headingWidth(_ width:CGFloat)->CGFloat {width-233-18}
    // The pin sits right after the heading, small and borderless, and still ends before 自动检测.
    init(_ data:Dashboard,width:CGFloat){
        self.data=data;Currency.unit=data.currency
        heatmapGeometry=Self.heatmapLayout(data);heatmapColumn=Self.labelColumn(data);heatmapLevels=data.heatmap?.levels ?? []
        super.init(frame:NSRect(x:0,y:0,width:width,height:Self.height(data)))
        setAccessibilityElement(true);setAccessibilityRole(.group);setAccessibilityLabel("用量概览")
        if let options=data.groupings,Self.groupingHeight(data)>0 {
            var x:CGFloat=8;let top=Self.summaryHeight(data)+Self.heatmapHeight(data)+Self.harnessHeaderHeight(data)+4
            for option in options {
                let tab=GroupTab(label:option.label);tab.chosen=option.key==(data.grouping ?? "tool")
                tab.setFrameOrigin(NSPoint(x:x,y:top));x+=tab.frame.width+2
                tab.invoke={[weak self] in self?.chooseGrouping(option.key)}
                groupTabs.append((option.key,tab));addSubview(tab)
            }
        }
        if data.collapsible {
            var y=Self.cardsTop(data)
            for p in data.cardList {
                let expanded=(data.expandedProviders ?? []).contains(p.id)
                let button=DisclosureButton(label:(expanded ? "收起 " : "展开 ")+p.name+" 明细",frame:NSRect(x:4,y:y+2,width:width-8,height:46))
                button.setAccessibilityValue(expanded ? "已展开" : "已折叠")
                button.invoke={[weak self] in self?.toggleProvider?(p.id,!expanded)}
                button.hovered={[weak self] over in self?.hoveredProvider=over ? p.id : nil;self?.needsDisplay=true}
                button.held={[weak self] down in self?.heldProvider=down ? p.id : nil;self?.needsDisplay=true}
                addSubview(button);y+=Self.providerHeight(p,data)
            }
        }
    }

    required init?(coder:NSCoder){fatalError()}
    var chooseGroup:((String)->Void)?;private var groupTabs:[(String,GroupTab)]=[]
    // The tab shows as chosen at once; the cards follow when the plugin answers.
    private func chooseGrouping(_ key:String){
        for (option,tab) in groupTabs {tab.chosen=option==key}
        if key != (data.grouping ?? "tool"){chooseGroup?(key)}
    }
    override func viewDidChangeEffectiveAppearance(){super.viewDidChangeEffectiveAppearance();needsDisplay=true}
    // Pointing at a square reads it out on the line under the squares.
    override func updateTrackingAreas(){
        super.updateTrackingAreas()
        if let heatmapTracking{removeTrackingArea(heatmapTracking)}
        heatmapTracking=nil
        guard data.heatmap != nil else{return}
        guard let geometry=heatmapGeometry else{return}
        let area=NSTrackingArea(rect:geometry.frame.insetBy(dx:-2,dy:-2),options:[.mouseMoved,.mouseEnteredAndExited,.activeAlways],owner:self)
        addTrackingArea(area);heatmapTracking=area
    }
    override func mouseMoved(with event:NSEvent){
        guard let h=data.heatmap,let geometry=heatmapGeometry else{return}
        hoverDay(Self.heatmapIndex(at:convert(event.locationInWindow,from:nil),h,geometry))
    }
    override func mouseEntered(with event:NSEvent){mouseMoved(with:event)}
    override func mouseExited(with event:NSEvent){hoverDay(nil)}
    private func hoverDay(_ day:Int?){
        guard day != hoveredDay else{return}
        hoveredDay=day;setNeedsDisplay(NSRect(x:0,y:Self.summaryHeight(data),width:bounds.width,height:Self.heatmapHeight(data)))
    }
    // A new reading of the same view rolls its figures from the old values to the new ones, and the cards and
    // Summary that grew glow in their colour for a moment, so a live (pinned) panel shows what just moved.
    private var motion:Timer?;private var grew:[String:Date]=[:];private var summaryGrew:Date?
    // Pinned, the note says when this reading arrived, to the second, in place of the minute it was read.
    var liveStamp:String?
    static let rollTime=0.7,glowTime=1.6
    func animate(from old:Dashboard){
        guard !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion,old.rangeKey==data.rangeKey,old.grouping==data.grouping else{return}
        let target=data,now=Date()
        for p in target.cardList {
            guard let before=old.cardList.first(where:{$0.id==p.id}) else{continue}
            if p.tokens>before.tokens || (p.cost ?? 0)>(before.cost ?? 0)+0.0001 || (p.credits ?? 0)>(before.credits ?? 0) {grew[p.id]=now}
        }
        func total(_ d:Dashboard)->Double {d.providers.compactMap{$0.summaryPart}.filter{$0.available}.reduce(0){$0+$1.tokens}}
        if total(target)>total(old) {summaryGrew=now}
        let rolling=Dashboard.moved(old,target)
        guard rolling || !grew.isEmpty || summaryGrew != nil else{return}
        if rolling {data=Dashboard.blend(old,target,0)}
        let timer=Timer(timeInterval:1.0/60,repeats:true){[weak self] timer in
            guard let self else{timer.invalidate();return}
            let elapsed=Date().timeIntervalSince(now),t=min(1,elapsed/Self.rollTime)
            if rolling {self.data=t>=1 ? target : Dashboard.blend(old,target,1-pow(1-t,3))}
            self.needsDisplay=true
            if elapsed>=max(Self.rollTime,Self.glowTime) {self.data=target;self.grew=[:];self.summaryGrew=nil;timer.invalidate();self.motion=nil;self.needsDisplay=true}
        }
        motion=timer;RunLoop.main.add(timer,forMode:.common)
    }
    override func viewWillMove(toWindow newWindow:NSWindow?){super.viewWillMove(toWindow:newWindow);if newWindow==nil{motion?.invalidate();motion=nil}}
    // How far a grown figure still glows: 1 just after the reading, 0 once it has settled.
    private func glow(_ start:Date?)->CGFloat {
        guard let start else{return 0}
        let t=Date().timeIntervalSince(start)/Self.glowTime
        return t>=1 ? 0 : CGFloat(pow(1-t,2))
    }
    private func glowing(_ base:NSColor,_ tint:NSColor,_ amount:CGFloat)->NSColor {
        amount<=0 ? base : (base.usingColorSpace(.sRGB).flatMap{b in tint.usingColorSpace(.sRGB).map{b.blended(withFraction:amount,of:$0) ?? base}} ?? base)
    }
    override func draw(_ dirtyRect:NSRect){
        let left:CGFloat=12,right=bounds.width-12,width=right-left
        let valid=summaryProviders.filter{$0.available},costs=valid.compactMap{$0.cost}
        let summaryBand=NSRect(x:0,y:0,width:bounds.width,height:Self.summaryHeight(data))
        if dirtyRect.intersects(summaryBand) {
        text(Self.heading(data),x:left,y:10,w:Self.headingWidth(bounds.width),size:14,weight:.semibold)
        if data.showSummary != false {
        let partial=valid.count != summaryProviders.count || valid.contains{$0.unpriced>0}
        let labels=[partial ? "已知 API 估算" : "API 估算","Token 用量","请求"]
        let values=[money(costs.isEmpty ? nil : costs.reduce(0,+)),valid.isEmpty ? "—" : tokens(valid.reduce(0){$0+$1.tokens}),valid.isEmpty ? "—" : String(valid.reduce(0){$0+$1.requests})]
        // Justified like the legend: left, centre and right thirds, each label above its value.
        // Each cell keeps 4pt clear of its neighbours: the first hugs the left edge, the last the right.
        let third=width/3,alignments:[NSTextAlignment]=[.left,.center,.right]
        let lift=glow(summaryGrew),tint=Self.totalColor(self)
        for i in 0..<3 {let x=left+third*CGFloat(i)+2*CGFloat(i);text(labels[i],x:x,y:40,w:third-4,size:10,color:.secondaryLabelColor,align:alignments[i]);Figure.draw(values[i],x:x,y:55,w:third-4,size:22,color:glowing(.labelColor,tint,lift*0.85),align:alignments[i])}
        drawSummaryChart(left:left,right:right)
        }
        }
        var y:CGFloat=Self.summaryHeight(data)
        if let heatmap=data.heatmap {
            if dirtyRect.intersects(NSRect(x:0,y:y,width:bounds.width,height:Self.heatmapHeight(data))){drawHeatmap(heatmap,left:left,right:right)}
            y+=Self.heatmapHeight(data)
        }
        y+=Self.harnessHeaderHeight(data)
        if let note=data.groupNote,Self.groupingHeight(data)>0 {text(note,x:right-150,y:y+9,w:150,size:9,color:.tertiaryLabelColor,align:.right)}
        y+=Self.groupingHeight(data)
        if data.cardList.isEmpty && !data.providers.isEmpty {text("此时段暂无调用",x:left,y:y+4,w:width,size:10.5,color:.secondaryLabelColor)}
        for p in data.cardList {
            let height=Self.providerHeight(p,data)
            if dirtyRect.intersects(NSRect(x:0,y:y,width:bounds.width,height:height)){if data.collapsible{drawCollapsibleProvider(p,y:y,left:left,right:right)}else{drawProvider(p,y:y,left:left,right:right)}}
            y+=height
        }
    }
    static func heatmapShades(_ view:NSView)->[NSColor] {
        let total=totalColor(view)
        return [NSColor.labelColor.withAlphaComponent(dark(view) ? 0.075 : 0.06)]+[0.3,0.52,0.76,1].map{total.withAlphaComponent($0)}
    }
    private func drawHeatmap(_ h:UsageHeatmap,left:CGFloat,right:CGFloat){
        guard let layout=heatmapGeometry else{return}
        let shades=Self.heatmapShades(self),levels=heatmapLevels,grid=layout.frame,cell=HeatmapLayout.cell
        for (row,name) in (h.rowLabels ?? []).enumerated() where row<h.rows && !name.isEmpty {
            text(name,x:left,y:grid.minY+CGFloat(row)*layout.rowPitch+cell/2-6,w:heatmapColumn-4,size:8,color:.secondaryLabelColor)
        }
        for index in 0..<min(h.tokens.count,h.rows*h.columns,levels.count) {
            let rect=layout.square(index),radius:CGFloat=1.6
            // Squares still to come are outlined, so the block stays whole and shows how far the day or week has gone.
            if index>=h.past {
                let outline=NSBezierPath(roundedRect:rect.insetBy(dx:0.4,dy:0.4),xRadius:radius,yRadius:radius);outline.lineWidth=0.8
                shades[0].setStroke();outline.stroke();continue
            }
            shades[levels[index]].setFill();NSBezierPath(roundedRect:rect,xRadius:radius,yRadius:radius).fill()
            if index==hoveredDay {
                let ring=NSBezierPath(roundedRect:rect.insetBy(dx:-1,dy:-1),xRadius:radius+0.6,yRadius:radius+0.6);ring.lineWidth=1.2
                NSColor.labelColor.withAlphaComponent(0.6).setStroke();ring.stroke()
            }
        }
        let line=grid.maxY+2,hintWidth=ceil((h.hint as NSString).size(withAttributes:[.font:NSFont.systemFont(ofSize:7.5)]).width)
        text(h.hint,x:right-hintWidth-2,y:line+1,w:hintWidth+2,size:7.5,color:.tertiaryLabelColor,align:.right)
        if let caption=Self.heatmapCaption(h,pointed:hoveredDay) {text(caption,x:grid.minX,y:line,w:right-grid.minX-hintWidth-10,size:9,color:.secondaryLabelColor)}
    }
    // Normal states say nothing; the slot shows a problem, or notes that change how to read the numbers: what a
    // gateway counted, in grey, then requests the price tables can't price yet, highlighted.
    static func notes(_ p:ProviderUsage)->[(String,Bool)] {
        if !p.available {return [(p.status=="运行正常" || p.status=="已同步" ? "数据暂不可用" : p.status,true)]}
        if p.status != "运行正常" && p.status != "已同步" {return [(p.status,true)]}
        return (p.subtitle.isEmpty ? [] : [(p.subtitle,false)])+(p.serviceNote.map{[($0,true)]} ?? [])+(p.priceNote.map{[($0,true)]} ?? [])
    }
    static func stats(_ p:ProviderUsage)->String {statsForms(p)[0]}
    // The counts line, longest first: a card that can't fit it rounds the cache share, then drops output, then the
    // sessions, before anything would be cut off.
    static func statsForms(_ p:ProviderUsage)->[String] {
        guard p.available else{return ["等待可用数据"]}
        // Cache and output say nothing when only Credits were recorded.
        if p.tokens==0,p.credits != nil {return ["\(p.requests) 请求 · \(p.sessions) 会话","\(p.requests) 请求"]}
        let requests="\(p.requests) 请求",output="输出 \(tokens(p.output))"
        func cache(_ digits:Int)->String {p.cache.map{String(format:"缓存 %.\(digits)f%%",$0*100)} ?? "缓存 —"}
        var extras:[String]=[]
        if p.id=="opencodex"{if let coverage=p.coverage,coverage<1{extras.append(String(format:"已定价 %.0f%%",coverage*100))}}else if !p.grouped{extras.append("\(p.sessions) 会话")}
        if p.cost != nil,let credits=p.credits {extras.append(creditsText(credits))}
        var forms:[String]=[]
        for parts in [[requests,cache(1),output]+extras,[requests,cache(0),output]+extras,[requests,cache(0)]+extras,[requests,cache(0)],[requests]] {
            let line=parts.joined(separator:" · ");if !forms.contains(line){forms.append(line)}
        }
        return forms
    }
    // A card's first line: the tool's mark and name, a note right after the name when there is one, the cost on the
    // right. The second line leads with its tokens, then the counts in grey.
    private func drawHeader(_ p:ProviderUsage,y:CGFloat,left:CGFloat,right:CGFloat){
        let nameFont=NSFont.systemFont(ofSize:13,weight:.semibold),mid=ToolMark.middle(of:nameFont,top:y+8)
        ToolMark.draw(p,in:NSRect(x:left,y:mid-7,width:14,height:14),color:accent(p.colour,self))
        let cost=p.available ? headline(p) : "—",costWidth=min(130,Figure.width(cost,size:16))
        let nameX=left+21,nameWidth=min(right-costWidth-14-nameX,ceil((p.name as NSString).size(withAttributes:[.font:nameFont]).width)+1)
        text(p.name,x:nameX,y:y+8,w:nameWidth,size:13,weight:.semibold)
        var noteX=nameX+nameWidth+7
        for (note,warning) in Self.notes(p) {
            let room=right-costWidth-10-noteX,width=min(room,ceil((note as NSString).size(withAttributes:[.font:NSFont.systemFont(ofSize:9.5)]).width)+1)
            guard room>12 else{break}
            text(note,x:noteX,y:y+10.5,w:width,size:9.5,color:warning ? .systemOrange : .secondaryLabelColor);noteX+=width+6
        }
        Figure.draw(cost,x:right-costWidth-2,y:y+5,w:costWidth+2,size:16,color:glowing(.labelColor,accent(p.colour,self),glow(grew[p.id])*0.9),align:.right)
    }
    private func drawFigures(_ p:ProviderUsage,y:CGFloat,left:CGFloat,right:CGFloat,stats:Bool){
        guard p.available else{text(Self.stats(p),x:left,y:y,w:right-left,size:10.5,color:.secondaryLabelColor);return}
        let lead=tokenLine(p),font=NSFont.systemFont(ofSize:11,weight:.semibold),leadWidth=ceil((lead as NSString).size(withAttributes:[.font:font]).width)+1
        text(lead,x:left,y:y,w:leadWidth,size:11,color:glowing(.labelColor,accent(p.colour,self),glow(grew[p.id])*0.9),weight:.semibold)
        if stats {
            let room=max(0,right-left-leadWidth-4),font=NSFont.systemFont(ofSize:10)
            let forms=Self.statsForms(p),line=forms.first{ceil(("· "+$0 as NSString).size(withAttributes:[.font:font]).width)<=room} ?? forms.last!
            text("· "+line,x:left+leadWidth+4,y:y+1,w:room,size:10,color:.secondaryLabelColor)
        }
    }
    private func drawModels(_ p:ProviderUsage,y:CGFloat,left:CGFloat,right:CGFloat,color c:NSColor){
        let width=right-left
        guard p.available else{return}
        if p.models.isEmpty{text("此时段暂无模型调用",x:left,y:y,w:width,size:10,color:.secondaryLabelColor)}
        // Each model's share is a short rounded bar of its own beside the figures, not a rule across the card.
        let shareX=right-34,amountX=shareX-62,barX=amountX-48,mid=ToolMark.middle(of:.systemFont(ofSize:10.5),top:0)
        for (i,m) in p.models.enumerated(){
            let top=y+CGFloat(i)*18
            var nameRoom=barX-left-10
            if let note=m.priceNote {
                let width=min(nameRoom*0.55,ceil((note as NSString).size(withAttributes:[.font:NSFont.systemFont(ofSize:9)]).width)+1)
                let nameWidth=min(nameRoom-width-5,ceil((modelName(m.name) as NSString).size(withAttributes:[.font:NSFont.systemFont(ofSize:10.5)]).width)+1)
                text(note,x:left+nameWidth+5,y:top+1.5,w:width,size:9,color:.systemOrange);nameRoom=nameWidth
            }
            text(modelName(m.name),x:left,y:top,w:nameRoom,size:10.5)
            let track=NSRect(x:barX,y:top+mid-2,width:44,height:4)
            NSColor.labelColor.withAlphaComponent(dark(self) ? 0.10 : 0.07).setFill();NSBezierPath(roundedRect:track,xRadius:2,yRadius:2).fill()
            c.withAlphaComponent(0.85).setFill();NSBezierPath(roundedRect:NSRect(x:barX,y:track.minY,width:max(4,44*CGFloat(min(1,m.share))),height:4),xRadius:2,yRadius:2).fill()
            text(modelAmount(m),x:amountX,y:top,w:58,size:10.5,align:.right,numeric:true)
            text(share(m.share),x:shareX,y:top,w:34,size:10,color:.secondaryLabelColor,align:.right,numeric:true)
        }
    }
    private func drawCollapsibleProvider(_ p:ProviderUsage,y:CGFloat,left:CGFloat,right:CGFloat){
        let expanded=(data.expandedProviders ?? []).contains(p.id)
        if hoveredProvider==p.id || heldProvider==p.id {
            let held=heldProvider==p.id
            NSColor.labelColor.withAlphaComponent(held ? (dark(self) ? 0.13 : 0.085) : (dark(self) ? 0.07 : 0.045)).setFill();NSBezierPath(roundedRect:NSRect(x:4,y:y+2,width:bounds.width-8,height:46),xRadius:8,yRadius:8).fill()
        }
        drawHeader(p,y:y,left:left,right:right-16)
        let arrow=NSBezierPath(),x=right-5,cy=ToolMark.middle(of:.systemFont(ofSize:13,weight:.semibold),top:y+8)
        arrow.move(to:NSPoint(x:x-3,y:cy-3));arrow.line(to:NSPoint(x:expanded ? x : x+0.5,y:cy));arrow.line(to:NSPoint(x:expanded ? x+3 : x-3,y:expanded ? cy-3 : cy+3))
        arrow.lineWidth=1.2;arrow.lineCapStyle = .round;arrow.lineJoinStyle = .round;NSColor.tertiaryLabelColor.setStroke();arrow.stroke()
        drawFigures(p,y:y+29,left:left+21,right:right,stats:expanded)
        if expanded {
            if let clients=Self.clientLine(p){text(clients,x:left+21,y:y+45,w:right-left-21,size:10,color:.secondaryLabelColor)}
            drawModels(p,y:y+52+Self.clientHeight(p),left:left+21,right:right,color:accent(p.colour,self))
        }
    }
    private func drawProvider(_ p:ProviderUsage,y:CGFloat,left:CGFloat,right:CGFloat){
        drawHeader(p,y:y,left:left,right:right)
        drawFigures(p,y:y+29,left:left+21,right:right,stats:true)
        if let clients=Self.clientLine(p){text(clients,x:left+21,y:y+45,w:right-left-21,size:10,color:.secondaryLabelColor)}
        drawModels(p,y:y+52+Self.clientHeight(p),left:left+21,right:right,color:accent(p.colour,self))
    }
    // Legend columns are justified: the first starts at the left edge, the last column's longest
    // label ends at the right edge, and column starts are evenly spaced. Four per row when they
    // fit, otherwise three, with rows as even as possible.
    static func legendLayout(_ labels:[String],width:CGFloat)->[[NSRect]] {
        guard !labels.isEmpty else{return []}
        let font=NSFont.systemFont(ofSize:8.5),gap:CGFloat=6
        let sizes=labels.map{ceil(($0 as NSString).size(withAttributes:[.font:font]).width)+16}
        func shape(_ most:Int)->(count:Int,pitch:CGFloat){
            let rows=(labels.count+most-1)/most,count=(labels.count+rows-1)/rows
            let last=stride(from:count-1,to:sizes.count,by:count).map{sizes[$0]}.max() ?? 0
            return (count,count>1 ? (width-last)/CGFloat(count-1) : 0)
        }
        func fits(_ s:(count:Int,pitch:CGFloat))->Bool{sizes.enumerated().allSatisfy{$0.offset%s.count==s.count-1 || $0.element+gap<=s.pitch}}
        var s=shape(4)
        if !fits(s){s=shape(3)}
        if !fits(s){s.pitch=width/CGFloat(s.count)}
        return stride(from:0,to:labels.count,by:s.count).map{start in
            (0..<min(s.count,labels.count-start)).map{c in
                let x=CGFloat(c)*s.pitch
                return NSRect(x:x,y:0,width:c==s.count-1 ? width-x : s.pitch-gap,height:12)
            }
        }
    }
    static func chartPositions(_ d:Dashboard)->[Double] {
        let n=d.dates.count
        if let values=d.chartPositions,values.count==n,values.allSatisfy({$0.isFinite}),zip(values,values.dropFirst()).allSatisfy({$0<$1}) {return values}
        return (0..<n).map{n>1 ? Double($0)/Double(n-1) : 0.5}
    }
    // Smoothed curves for each series followed by their total. Slopes and limiters are shared,
    // so the smoothed total equals the sum of the provider curves at every point, including
    // between days, and no line overshoots its observations. Needs at least two points.
    static func smoothCurves(_ series:[[Double]],positions:[Double],plot:NSRect,ceiling:Double)->[NSBezierPath] {
        let n=positions.count
        let totals=(0..<n).map{i in series.reduce(0){$0+$1[i]}}
        let intervals=(0..<n-1).map{positions[$0+1]-positions[$0]}
        func slopes(_ values:[Double])->[Double]{
            let d=(0..<n-1).map{(values[$0+1]-values[$0])/intervals[$0]}
            var result=[Double](repeating:0,count:n)
            result[0]=d[0];result[n-1]=d[n-2]
            if n>2{for i in 1..<n-1{result[i]=(d[i-1]+d[i])/3}}
            return result
        }
        let shapeSeries=series+[totals]
        var tangents=shapeSeries.map{slopes($0)}
        for i in 0..<n {
            var factor=1.0
            for (j,values) in shapeSeries.enumerated(){
                let t=tangents[j][i]
                for k in [i-1,i] where k>=0 && k<n-1 {
                    let d=(values[k+1]-values[k])/intervals[k]
                    if d==0 || t*d<0{factor=0}else if abs(t)>0{factor=min(factor,abs(3*d/t))}
                }
            }
            for j in tangents.indices{tangents[j][i]*=factor}
        }
        for i in 0..<n-1 {
            var factor=1.0
            for (j,values) in shapeSeries.enumerated(){let delta=values[i+1]-values[i];if delta != 0 {let a=tangents[j][i]*intervals[i]/delta,b=tangents[j][i+1]*intervals[i]/delta,s=a*a+b*b;if s>9{factor=min(factor,3/sqrt(s))}}}
            for j in tangents.indices{tangents[j][i]*=factor;tangents[j][i+1]*=factor}
        }
        let scale=plot.height/CGFloat(ceiling)
        return zip(shapeSeries,tangents).map{values,slopes in
            let points=values.enumerated().map{NSPoint(x:plot.minX+CGFloat(positions[$0.offset])*plot.width,y:plot.maxY-CGFloat($0.element)*scale)}
            let curve=NSBezierPath();curve.move(to:points[0])
            for i in 0..<n-1{let dt=CGFloat(intervals[i]),dx=plot.width*dt;curve.curve(to:points[i+1],controlPoint1:NSPoint(x:points[i].x+dx/3,y:points[i].y-CGFloat(slopes[i])*dt*scale/3),controlPoint2:NSPoint(x:points[i+1].x-dx/3,y:points[i+1].y+CGFloat(slopes[i+1])*dt*scale/3))}
            return curve
        }
    }
    private func drawSummaryChart(left:CGFloat,right:CGFloat){
        let width=right-left,n=data.dates.count
        guard n>0 else{return}
        let series=summaryProviders.map{p in (0..<n).map{i in i<p.history.count ? max(0,p.history[i]) : 0}}
        let totals=(0..<n).map{i in series.reduce(0){$0+$1[i]}}
        let totalColor=Self.totalColor(self),(ceiling,axis)=Self.chartAxis(data)
        let labels=Self.legendLabels(data),colors=[totalColor]+summaryProviders.map{accent($0.id,self)}
        var index=0
        for (row,frame) in Self.legendLayout(labels,width:width).enumerated(){
            for cell in frame{
                let x=left+cell.minX,y=CGFloat(row)*17
                colors[index].setFill();NSBezierPath(roundedRect:NSRect(x:x,y:96+y,width:11,height:index==0 ? 3 : 2),xRadius:1.5,yRadius:1.5).fill()
                text(labels[index],x:x+16,y:90+y,w:cell.width-16,size:8.5,color:.secondaryLabelColor)
                index+=1
            }
        }
        // The live chart draws itself over this space while the panel is pinned.
        if live != nil {return}
        NSGraphicsContext.saveGraphicsState()
        defer{NSGraphicsContext.restoreGraphicsState()}
        NSGraphicsContext.current?.cgContext.translateBy(x:0,y:Self.chartExtra(data))
        // The chart's text keeps to the same edges as the figures above: axis labels flush left in the column
        // the heatmap's row names share, the plot right after it, and the note and last time label flush right.
        // The plot has no backdrop of its own, so it sits as lightly as the squares below it.
        let column=Self.labelColumn(data)
        let plot=NSRect(x:left+column,y:127,width:right-left-column,height:78),cardFrame=NSRect(x:plot.minX,y:119,width:plot.width,height:90)
        for (f,label) in zip([CGFloat(0),0.5,1],axis){
            let yy=plot.maxY-f*plot.height
            text(label,x:left,y:yy-5,w:column-4,size:8,color:.secondaryLabelColor,numeric:true)
            line(NSPoint(x:plot.minX,y:yy),NSPoint(x:plot.maxX,y:yy),color:NSColor.separatorColor.withAlphaComponent(f==0 ? 0.3 : 0.14),width:0.5)
        }
        // Partial data and when the figures were read share a faint note just above the card, clear of its edge.
        let note=[data.chartStatus,liveStamp ?? data.updated ?? ""].filter{!$0.isEmpty}.joined(separator:" · ")
        if !note.isEmpty {text(note,x:cardFrame.minX,y:cardFrame.minY-12,w:cardFrame.width,size:7.5,color:.tertiaryLabelColor,align:.right)}
        if n==1 {
            for (i,values) in series.enumerated(){let y=plot.maxY-CGFloat(values[0]/ceiling)*plot.height;accent(summaryProviders[i].id,self).setFill();NSBezierPath(ovalIn:NSRect(x:plot.midX-2.5,y:y-2.5,width:5,height:5)).fill()}
            let y=plot.maxY-CGFloat(totals[0]/ceiling)*plot.height;totalColor.setFill();NSBezierPath(ovalIn:NSRect(x:plot.midX-3,y:y-3,width:6,height:6)).fill()
            text(data.dates[0],x:plot.midX-30,y:212,w:60,size:8.5,color:.secondaryLabelColor,align:.center);return
        }
        let positions=Self.chartPositions(data)
        let paths=Self.smoothCurves(series,positions:positions,plot:plot,ceiling:ceiling),totalPath=paths.last!
        let area=totalPath.copy() as! NSBezierPath;area.line(to:NSPoint(x:plot.maxX,y:plot.maxY));area.line(to:NSPoint(x:plot.minX,y:plot.maxY));area.close()
        NSGraphicsContext.saveGraphicsState();area.addClip();NSGradient(starting:totalColor.withAlphaComponent(dark(self) ? 0.16 : 0.11),ending:totalColor.withAlphaComponent(0))?.draw(from:NSPoint(x:plot.midX,y:plot.minY),to:NSPoint(x:plot.midX,y:plot.maxY),options:[]);NSGraphicsContext.restoreGraphicsState()
        for (i,curve) in paths.enumerated(){
            let total=i==paths.count-1
            curve.lineWidth=total ? 1.8 : 1.2;curve.lineCapStyle = .round;curve.lineJoinStyle = .round
            (total ? totalColor : accent(summaryProviders[i].id,self).withAlphaComponent(0.8)).setStroke();curve.stroke()
        }
        let end=NSPoint(x:plot.minX+CGFloat(positions[n-1])*plot.width,y:plot.maxY-CGFloat(totals[n-1]/ceiling)*plot.height)
        totalColor.withAlphaComponent(0.18).setFill();NSBezierPath(ovalIn:NSRect(x:end.x-5,y:end.y-5,width:10,height:10)).fill()
        totalColor.setFill();NSBezierPath(ovalIn:NSRect(x:end.x-2.5,y:end.y-2.5,width:5,height:5)).fill()
        let middle=(1..<n-1).filter{positions[$0]>=0.25 && positions[$0]<=0.75}.min{abs(positions[$0]-0.5)<abs(positions[$1]-0.5)}
        let labelIndices=[0]+(middle.map{[$0]} ?? [])+[n-1]
        for i in labelIndices{let label=data.dates[i];let x=plot.minX+CGFloat(positions[i])*plot.width;text(label,x:i==0 ? x : (i==n-1 ? right-48 : x-25),y:212,w:i==n-1 ? 48 : 50,size:8.5,color:.secondaryLabelColor,align:i==0 ? .left : (i==n-1 ? .right : .center))}
    }

}
// The module list as magpie lays out its provider page: a wide, flat grid under plain section titles, every
// module in view at once, no frames. Ticked modules come first in each section; the order is fixed when the
// list opens, so ticking one never moves cells under the pointer.
enum ModuleGrid {
    static let columns=3,cell:CGFloat=32,noted:CGFloat=44,title:CGFloat=34,gap:CGFloat=6,text:CGFloat=54,spacing:CGFloat=4,width:CGFloat=588
    struct Section {var title:String?;var cells:[Entry]}
    static func sections(_ entries:[Entry],leading:Set<String>? = nil)->[Section] {
        var result:[Section]=[];var current=Section(title:nil,cells:[])
        func close(){
            defer{current=Section(title:nil,cells:[])}
            guard !current.cells.isEmpty else{return}
            let first={(e:Entry)->Bool in e.isModule && (leading.map{$0.contains(e.key)} ?? (e.checked==true))}
            current.cells=current.cells.filter(first)+current.cells.filter{!first($0)}
            result.append(current)
        }
        for e in entries {
            if e.separator==true{close()}
            else if e.section==true{close();current.title=e.label}
            else{current.cells.append(e)}
        }
        close();return result
    }
    static func rows(_ s:Section)->[[Entry]] {stride(from:0,to:s.cells.count,by:columns).map{Array(s.cells[$0..<min($0+columns,s.cells.count)])}}
    static func rowHeight(_ row:[Entry])->CGFloat {row.contains{$0.detail != nil} ? noted : cell}
    static func height(_ s:Section)->CGFloat {(s.title==nil ? 0 : title)+rows(s).reduce(0){$0+rowHeight($1)}}
    static func height(_ entries:[Entry],leading:Set<String>? = nil)->CGFloat {sections(entries,leading:leading).reduce(0){$0+height($1)+gap}}
    static func cellWidth(_ inner:CGFloat)->CGFloat {floor((inner-spacing*CGFloat(columns-1))/CGFloat(columns))}
}
enum MenuLayout {
    static func configurationClick(_ e:Entry,x:CGFloat,width:CGFloat)->Bool {
        e.isModule && !e.children.isEmpty && x>=width-32 && x<=width
    }
    static func configurationHover(_ e:Entry,x:CGFloat)->Bool { !e.isModule || x>=36 }
    static func hasIcon(_ e:Entry)->Bool {e.image != nil || e.symbol != nil || e.checked != nil || e.statusLight != nil}
    static func column(_ entries:[Entry])->CGFloat {entries.contains(where:hasIcon) ? 36 : 12}
    // A checkbox row with its own icon (a module) shows both, so its name starts one icon further in.
    static func marked(_ e:Entry)->Bool {e.checked != nil && (e.image != nil || e.symbol != nil)}
    static func left(_ e:Entry,_ entries:[Entry])->CGFloat {column(entries)+(marked(e) ? 20 : 0)}
    static func labelFont(_ e:Entry)->NSFont {.systemFont(ofSize:e.section==true ? 10.5 : (e.actionable ? 12 : 10.5),weight:e.section==true ? .medium : .regular)}
    static let valueFont=NSFont.monospacedDigitSystemFont(ofSize:11,weight:.regular)
    static let detailFont=NSFont.systemFont(ofSize:10)
    static func width(_ value:String,font:NSFont)->CGFloat {ceil((value as NSString).size(withAttributes:[.font:font]).width)}
    static func height(_ value:String,font:NSFont,width:CGFloat)->CGFloat {
        ceil((value as NSString).boundingRect(with:NSSize(width:max(1,width),height:1000),options:[.usesLineFragmentOrigin,.usesFontLeading],attributes:[.font:font]).height)
    }
    static func configured(_ value:String?)->Bool? {
        if value=="已配置"{return true}
        if value=="未配置"{return false}
        return nil
    }
    static func valueWidth(_ e:Entry)->CGFloat {e.value.map{width($0,font:valueFont)+(e.attention==true ? 16 : configured($0) != nil ? 12 : 0)} ?? 0}
    static func trailing(_ e:Entry)->CGFloat {e.children.isEmpty ? 12 : 32}
    static func labelWidth(_ e:Entry,width:CGFloat,left:CGFloat)->CGFloat {max(24,width-left-trailing(e)-valueWidth(e)-(e.value == nil ? 0 : 10))}
    // A value too wide for its row gives way to its shorter forms, so neither the title nor the value is cut.
    static func fitted(_ e:Entry,width:CGFloat,left:CGFloat)->Entry {
        guard let value=e.value,let shorter=e.fallbacks,!shorter.isEmpty else{return e}
        let room=width-left-trailing(e)-10-MenuLayout.width(e.label,font:labelFont(e));var entry=e
        for form in [value]+shorter {entry.value=form;if valueWidth(entry)<=room{break}}
        return entry
    }
    static func rowHeight(_ e:Entry,width:CGFloat,left:CGFloat)->CGFloat {
        let e=fitted(e,width:width,left:left)
        if e.separator==true{return 12}
        if e.section==true{return 25}
        let titleHeight=height(e.label,font:labelFont(e),width:labelWidth(e,width:width,left:left))
        let detailHeight=e.detail.map{height($0,font:detailFont,width:width-left-trailing(e))+3} ?? 0
        return max(e.actionable ? 27 : 21,titleHeight+detailHeight+(e.detail == nil ? 8 : 12))+(gauge(e) ? 8 : 0)
    }
    // A homepage row that carries a usage share shows it as a thin bar under its name, as Token Monitor's rows do.
    static func gauge(_ e:Entry)->Bool {!e.children.isEmpty && DetailStyle.share(e) != nil}
    static func flyoutWidth(_ entry:Entry)->CGFloat {
        let left=column(entry.children)
        var needed=width(entry.label,font:.systemFont(ofSize:11,weight:.medium))+left+28
        for e in entry.children where e.separator != true {
            let title=width(e.label,font:labelFont(e))+valueWidth(e)+(e.value == nil ? 0 : 10)
            let detail=e.detail.map{min(216,width($0,font:detailFont))} ?? 0
            needed=max(needed,max(title,detail)+left+(marked(e) ? 20 : 0)+trailing(e)+16)
        }
        return min(296,max(224,ceil(needed/8)*8))
    }
    // Prefer the same side as the previous level; never bounce back over the root if there is room.
    static func flyoutX(parent:NSRect,ancestor:NSRect?,width:CGFloat,screen:NSRect)->CGFloat {
        let left=parent.minX-width-5,right=parent.maxX+5
        let preferLeft=ancestor.map{parent.minX<$0.minX} ?? false
        let candidates=preferLeft ? [left,right] : [right,left]
        if let fits=candidates.first(where:{$0>=screen.minX+4 && $0+width<=screen.maxX-4}){return fits}
        return max(screen.minX+4,min(candidates[0],screen.maxX-width-4))
    }
    static func topmostIndex(at point:NSPoint,frames:[NSRect])->Int? {
        frames.indices.reversed().first{frames[$0].contains(point)}
    }
    static func highlighted(pointerInside:Bool,expanded:Bool,yielded:Bool)->Bool {pointerInside || (expanded && !yielded)}
    // Movement from `from` to `to` heads into the open flyout through the triangle spanned by
    // its near edge, so rows crossed on the way must not replace it (native menu safe zone).
    static func aiming(from:NSPoint,to:NSPoint,flyout:NSRect)->Bool {
        // A slow drift is choosing a row, not travelling into the flyout.
        guard hypot(to.x-from.x,to.y-from.y)>=4 else{return false}
        let edge=flyout.minX>=from.x ? flyout.minX : flyout.maxX
        guard abs(edge-to.x)<abs(edge-from.x)-0.5 else{return false}
        let top=NSPoint(x:edge,y:flyout.maxY+4),bottom=NSPoint(x:edge,y:flyout.minY-4)
        func side(_ a:NSPoint,_ b:NSPoint)->CGFloat {(b.x-a.x)*(to.y-a.y)-(b.y-a.y)*(to.x-a.x)}
        let s1=side(from,top),s2=side(top,bottom),s3=side(bottom,from)
        return (s1>=0 && s2>=0 && s3>=0) || (s1<=0 && s2<=0 && s3<=0)
    }
    static func draw(_ value:String,in rect:NSRect,font:NSFont,color:NSColor,alignment:NSTextAlignment = .left){
        let style=NSMutableParagraphStyle();style.alignment=alignment;style.lineBreakMode = .byWordWrapping
        (value as NSString).draw(with:rect,options:[.usesLineFragmentOrigin,.usesFontLeading],attributes:[.font:font,.foregroundColor:color,.paragraphStyle:style],context:nil)
    }
}

final class Row:NSButton {
    override func acceptsFirstMouse(for event:NSEvent?)->Bool{true}
    let entry:Entry;var pointerAllowed:(()->Bool)?;var invoke:((Entry,Row)->Void)?;var hover:((Entry,Row)->Void)?
    var expanded=false {didSet{needsDisplay=true}}
    // The pointer is on a sibling row, so this row's open flyout no longer owns the highlight.
    var yielded=false {didSet{if yielded != oldValue{needsDisplay=true}}}
    var columnInset:CGFloat=12
    var carded=false {didSet{needsDisplay=true}}
    private(set) var configurationClick=false
    private var hovering=false;private var tracking:NSTrackingArea?;private var glyph:NSImage?
    init(_ entry:Entry,frame:NSRect){
        self.entry=entry;super.init(frame:frame);title=entry.label;isBordered=false;setButtonType(.momentaryChange)
        target=self;action=#selector(activate);isEnabled=entry.actionable
        setAccessibilityLabel(entry.label+(entry.value.map{", "+$0} ?? "")+(entry.detail.map{", "+$0} ?? ""))
        if entry.isModule && !entry.children.isEmpty{setAccessibilityHelp("点击切换主页显示；悬停或点击右侧箭头配置连接") }
        if let raw=entry.image,let data=Data(base64Encoded:raw),let img=NSImage(data:data){glyph=img;glyph?.isTemplate=entry.template ?? false}
        else if let symbol=entry.symbol{glyph=NSImage(systemSymbolName:symbol,accessibilityDescription:nil)}
    }
    required init?(coder:NSCoder){fatalError()}
    private func drawGlyph(_ glyph:NSImage,x:CGFloat){drawGlyph(glyph,in:NSRect(x:x,y:(bounds.height-14)/2,width:14,height:14))}
    private func drawGlyph(_ glyph:NSImage,in rect:NSRect){
        if glyph.isTemplate {
            let image=NSImage(size:rect.size);image.lockFocus();glyph.draw(in:NSRect(origin:.zero,size:rect.size));(dark(self) ? NSColor(calibratedWhite:0.9,alpha:1) : NSColor(calibratedWhite:0.18,alpha:1)).setFill();NSRect(origin:.zero,size:rect.size).fill(using:.sourceIn);image.unlockFocus();image.draw(in:rect)
        } else {glyph.draw(in:rect)}
    }
    // A grid cell, flat as magpie's: checkbox, icon, name and a green dot when configured; the arrow to the
    // module's settings shows only under the pointer.
    private func drawCarded(highlighted:Bool){
        if (highlighted || isHighlighted) && isEnabled {NSColor.labelColor.withAlphaComponent(isHighlighted ? (dark(self) ? 0.16 : 0.1) : (dark(self) ? 0.08 : 0.05)).setFill();NSBezierPath(roundedRect:bounds.insetBy(dx:0,dy:1),xRadius:7,yRadius:7).fill()}
        let line:CGFloat=entry.detail==nil ? bounds.midY : 15
        if let checked=entry.checked {
            let box=NSRect(x:8,y:line-6.5,width:13,height:13),outline=NSBezierPath(roundedRect:box,xRadius:3.5,yRadius:3.5)
            if checked {
                starAccent.setFill();outline.fill();NSColor.white.setStroke()
                let tick=NSBezierPath();tick.move(to:NSPoint(x:box.minX+3,y:box.midY));tick.line(to:NSPoint(x:box.minX+5.5,y:box.maxY-3.5));tick.line(to:NSPoint(x:box.maxX-3,y:box.minY+3.5))
                tick.lineWidth=1.6;tick.lineCapStyle = .round;tick.lineJoinStyle = .round;tick.stroke()
            } else {NSColor.secondaryLabelColor.withAlphaComponent(0.55).setStroke();outline.lineWidth=1;outline.stroke()}
        }
        if let glyph{drawGlyph(glyph,in:NSRect(x:29,y:line-8,width:16,height:16))}
        let font=NSFont.systemFont(ofSize:12.5,weight:.medium),left=ModuleGrid.text,trailing:CGFloat=entry.children.isEmpty ? 8 : 22
        // The name keeps one line: it may use the arrow's place when there is no arrow, and ends in … if still too long.
        let room=bounds.width-left-(entry.children.isEmpty ? 6 : 18)-(entry.value=="已配置" ? 12 : 0),name=min(room,MenuLayout.width(entry.label,font:font))
        let top=line-(ToolMark.middle(of:font,top:0)),style=NSMutableParagraphStyle();style.lineBreakMode = .byTruncatingTail
        (entry.label as NSString).draw(in:NSRect(x:left,y:top,width:name,height:ceil(font.ascender-font.descender)+2),withAttributes:[.font:font,.foregroundColor:NSColor.labelColor,.paragraphStyle:style])
        if entry.value=="已配置" {statusGreen(self).setFill();NSBezierPath(ovalIn:NSRect(x:left+name+6,y:line-3,width:6,height:6)).fill()}
        if let detail=entry.detail {MenuLayout.draw(detail,in:NSRect(x:left,y:line+9,width:bounds.width-left-trailing,height:14),font:MenuLayout.detailFont,color:.secondaryLabelColor)}
        if !entry.children.isEmpty && (highlighted || expanded) {
            let p=NSBezierPath();p.move(to:NSPoint(x:bounds.width-14,y:line-4));p.line(to:NSPoint(x:bounds.width-10,y:line));p.line(to:NSPoint(x:bounds.width-14,y:line+4))
            p.lineWidth=1.2;p.lineJoinStyle = .round;p.lineCapStyle = .round;NSColor.secondaryLabelColor.setStroke();p.stroke()
        }
    }
    @objc private func activate(){invoke?(entry,self)}
    override func viewDidChangeEffectiveAppearance(){super.viewDidChangeEffectiveAppearance();needsDisplay=true}
    override func updateTrackingAreas(){super.updateTrackingAreas();if let tracking{removeTrackingArea(tracking)};tracking=NSTrackingArea(rect:bounds,options:[.mouseEnteredAndExited,.activeAlways,.inVisibleRect],owner:self,userInfo:nil);addTrackingArea(tracking!)}
    override func mouseDown(with event:NSEvent){
        guard pointerAllowed?() ?? true else{return}
        let x=convert(event.locationInWindow,from:nil).x
        configurationClick=carded ? entry.isModule && !entry.children.isEmpty && x>=26 : MenuLayout.configurationClick(entry,x:x,width:bounds.width)
        defer{configurationClick=false}
        if isEnabled {Press.track(self,scale:carded ? 0.96 : 0.985){super.mouseDown(with:event)}} else {super.mouseDown(with:event)}
    }
    override func mouseEntered(with event:NSEvent){guard pointerAllowed?() ?? true else{return};hovering=true;needsDisplay=true;hover?(entry,self)}
    override func mouseExited(with event:NSEvent){hovering=false;needsDisplay=true}
    override func draw(_ dirtyRect:NSRect){
        let pointerInside=(pointerAllowed?() ?? true) && (window.map{$0.isVisible && $0.convertToScreen(convert(bounds,to:nil)).contains(NSEvent.mouseLocation)} ?? false)
        if carded {drawCarded(highlighted:MenuLayout.highlighted(pointerInside:pointerInside,expanded:expanded,yielded:yielded));return}
        if (MenuLayout.highlighted(pointerInside:pointerInside,expanded:expanded,yielded:yielded) || isHighlighted) && isEnabled {
            // Held down, the row darkens beyond its hover tint.
            NSColor.labelColor.withAlphaComponent(isHighlighted ? (dark(self) ? 0.14 : 0.09) : (dark(self) ? 0.065 : 0.045)).setFill();NSBezierPath(roundedRect:bounds,xRadius:7,yRadius:7).fill()
        }
        let left=columnInset
        if let checked=entry.checked {
            let box=NSRect(x:13,y:(bounds.height-12)/2,width:12,height:12)
            let radio=entry.radio==true
            let outline=NSBezierPath(roundedRect:box,xRadius:radio ? 6 : 3,yRadius:radio ? 6 : 3)
            if checked {
                starAccent.setFill();outline.fill()
                NSColor.white.setStroke();NSColor.white.setFill()
                if radio {NSBezierPath(ovalIn:box.insetBy(dx:4,dy:4)).fill()}
                else {let tick=NSBezierPath();tick.move(to:NSPoint(x:box.minX+2.5,y:box.midY));tick.line(to:NSPoint(x:box.minX+5,y:box.maxY-3));tick.line(to:NSPoint(x:box.maxX-2.5,y:box.minY+3));tick.lineWidth=1.5;tick.lineCapStyle = .round;tick.lineJoinStyle = .round;tick.stroke()}
            } else {NSColor.secondaryLabelColor.withAlphaComponent(0.5).setStroke();outline.lineWidth=1;outline.stroke()}
            if let glyph{drawGlyph(glyph,x:33)}
        } else if let glyph {drawGlyph(glyph,x:12)}

        let trailing=MenuLayout.trailing(entry),font=MenuLayout.labelFont(entry)
        let color:NSColor=entry.action?.first=="stop" ? .systemRed : (entry.actionable || entry.usageLevel != nil || entry.checked==true ? .labelColor : .secondaryLabelColor)
        let labelWidth=MenuLayout.labelWidth(entry,width:bounds.width,left:left)
        let titleHeight=MenuLayout.height(entry.label,font:font,width:labelWidth)
        let detailHeight=entry.detail.map{MenuLayout.height($0,font:MenuLayout.detailFont,width:bounds.width-left-trailing)} ?? 0
        let gauge=MenuLayout.gauge(entry)
        let blockHeight=titleHeight+(entry.detail == nil ? 0 : detailHeight+3)+(gauge ? 8 : 0)
        let top=(bounds.height-blockHeight)/2
        if let state=entry.statusLight {drawStatusLight(state,centre:NSPoint(x:19,y:top+titleHeight/2),in:self)}
        MenuLayout.draw(entry.label,in:NSRect(x:left,y:top,width:labelWidth,height:titleHeight),font:font,color:color)
        // With a detail line or a bar below, the value sits on the title line so the two never overlap.
        let line=entry.detail == nil && !gauge ? bounds.midY : top+titleHeight/2
        if gauge,let share=DetailStyle.share(entry) {
            let bar=NSRect(x:left,y:top+titleHeight+4,width:bounds.width-left-trailing,height:3),night=dark(self)
            NSColor.labelColor.withAlphaComponent(night ? 0.12 : 0.08).setFill();NSBezierPath(roundedRect:bar,xRadius:1.5,yRadius:1.5).fill()
            DetailStyle.barColor(entry.usageLevel).setFill()
            NSBezierPath(roundedRect:NSRect(x:bar.minX,y:bar.minY,width:max(3,bar.width*CGFloat(share)),height:bar.height),xRadius:1.5,yRadius:1.5).fill()
        }
        if let value=entry.value {
            let valueWidth=MenuLayout.valueWidth(entry)
            let valueRect=NSRect(x:bounds.width-trailing-valueWidth,y:line-10,width:valueWidth,height:20)
            let textHeight=MenuLayout.height(value,font:MenuLayout.valueFont,width:valueWidth)
            if entry.attention==true {
                starAccent.withAlphaComponent(dark(self) ? 0.22 : 0.10).setFill()
                NSBezierPath(roundedRect:valueRect,xRadius:7,yRadius:7).fill()
                MenuLayout.draw(value,in:NSRect(x:valueRect.minX+8,y:line-textHeight/2,width:valueRect.width-16,height:textHeight),font:MenuLayout.valueFont,color:dark(self) ? accent("opencodex",self) : starAccent,alignment:.center)
            }else if let configured=MenuLayout.configured(value){
                drawStatusLight(configured ? "ok" : "unknown",centre:NSPoint(x:valueRect.minX+3.5,y:line),in:self)
                MenuLayout.draw(value,in:NSRect(x:valueRect.minX+12,y:line-textHeight/2,width:valueRect.width-12,height:textHeight),font:MenuLayout.valueFont,color:.secondaryLabelColor,alignment:.right)
            }else{
                MenuLayout.draw(value,in:NSRect(x:valueRect.minX,y:line-textHeight/2,width:valueRect.width,height:textHeight),font:MenuLayout.valueFont,color:usageColor(entry.usageLevel) ?? (entry.actionable ? .labelColor : .secondaryLabelColor),alignment:.right)
            }
        }
        if let detail=entry.detail {MenuLayout.draw(detail,in:NSRect(x:left,y:top+titleHeight+(gauge ? 8 : 0)+3,width:bounds.width-left-trailing,height:detailHeight),font:MenuLayout.detailFont,color:.secondaryLabelColor)}

        if !entry.children.isEmpty{let p=NSBezierPath();p.move(to:NSPoint(x:bounds.width-19,y:line-4));p.line(to:NSPoint(x:bounds.width-15,y:line));p.line(to:NSPoint(x:bounds.width-19,y:line+4));p.lineWidth=1.2;p.lineJoinStyle = .round;p.lineCapStyle = .round;NSColor.labelColor.setStroke();p.stroke()}
    }
}
// MARK: - Live chart
// Pinned, the panel is being watched while the user works, so the chart zooms in on now: what Summary's tools added
// in the last minutes, read every couple of seconds, its scale fitted to that, the line flowing left as time passes.
final class LiveFeed {
    struct Sample {var at:Date;var total:Double;var cost:Double;var parts:[String:Double]}
    private(set) var samples:[Sample]=[]
    var started=Date()
    private var rangeKey:String?
    static let longest:TimeInterval=600,shortest:TimeInterval=60
    func add(_ dashboard:Dashboard,at:Date){
        if dashboard.rangeKey != rangeKey {samples=[];rangeKey=dashboard.rangeKey}
        let parts=dashboard.providers.compactMap{$0.summaryPart}.filter{$0.available}
        var sample=Sample(at:at,total:parts.reduce(0){$0+$1.tokens},cost:parts.reduce(0){$0+($1.cost ?? 0)},parts:[:])
        for part in parts {sample.parts[part.id]=part.tokens}
        // A new day (or a source dropping out) starts the line again rather than drawing a fall.
        if let last=samples.last,sample.total<last.total-1 {samples=[]}
        if let last=samples.last,abs(at.timeIntervalSince(last.at))<0.5 {samples[samples.count-1]=sample} else {samples.append(sample)}
        samples.removeAll{at.timeIntervalSince($0.at)>Self.longest+60}
    }
    // The window on screen: from the pin's first minute, widening to the last ten.
    func span(_ now:Date)->TimeInterval {min(Self.longest,max(Self.shortest,now.timeIntervalSince(started)))}
    // Tokens per minute over the last minute of samples.
    func rate(_ now:Date)->Double? {
        guard let last=samples.last,let first=samples.last(where:{now.timeIntervalSince($0.at)>=60}) ?? samples.first,last.at>first.at else{return nil}
        return (last.total-first.total)/last.at.timeIntervalSince(first.at)*60
    }
}

final class LiveChart:NSView {
    let feed:LiveFeed
    var colors:[String:NSColor]=[:],total:NSColor = .labelColor,column:CGFloat=40,order:[String]=[]
    private var timer:Timer?;private var ceiling:Double=0;private var seen:Date?;private var ripple:Date?;private var born=Date()
    init(feed:LiveFeed){self.feed=feed;super.init(frame:.zero)}
    // A round top for the scale, close above the highest value so the line fills the chart.
    static func nice(_ value:Double)->Double {
        let power=pow(10,floor(log10(max(1,value))))
        return ([1,1.2,1.5,2,2.5,3,4,5,6,8,10].map{$0*power}.first{$0>=value}) ?? value
    }
    static func compact(_ value:Double)->String {
        let (number,unit)=value>=1e9 ? (value/1e9,"B") : value>=1e6 ? (value/1e6,"M") : value>=1e3 ? (value/1e3,"K") : (value,"")
        return (number.rounded()==number ? String(Int(number)) : String(format:"%.1f",number))+unit
    }
    required init?(coder:NSCoder){fatalError()}
    override var isFlipped:Bool{true}
    override func viewDidMoveToWindow(){
        super.viewDidMoveToWindow()
        timer?.invalidate();timer=nil
        guard window != nil else{return}
        let motion = !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        let tick=Timer(timeInterval:motion ? 1.0/30 : 1,repeats:true){[weak self] _ in self?.needsDisplay=true}
        timer=tick;RunLoop.main.add(tick,forMode:.common)
    }
    // The tokens each line has added since the window's left edge, at a moment: the latest sample by then.
    private func value(_ id:String?,at time:Date,base:LiveFeed.Sample)->Double {
        let sample=feed.samples.last{$0.at<=time} ?? base
        return id.map{(sample.parts[$0] ?? 0)-(base.parts[$0] ?? 0)} ?? sample.total-base.total
    }
    override func draw(_ dirtyRect:NSRect){
        let now=Date(),night=dark(self),span=feed.span(now),start=now.addingTimeInterval(-span)
        let plot=NSRect(x:column,y:24,width:bounds.width-column,height:bounds.height-46)
        let samples=feed.samples
        if let last=samples.last?.at,last != seen {if seen != nil{ripple=now};seen=last}
        // The scale eases to the window's growth, never below a small floor so an idle line lies flat on zero.
        let base=samples.last{$0.at<=start} ?? samples.first
        let points=samples.filter{$0.at>start}
        let target=Self.nice(max(2_000,(points.map{$0.total-(base?.total ?? 0)}.max() ?? 0)*1.04))
        ceiling = ceiling==0 ? target : ceiling+(target-ceiling)*0.12
        // Header: what the window added, its rate, and when the last reading came in.
        let added=base.map{(samples.last?.total ?? $0.total)-$0.total} ?? 0
        let minutes=Int((span/60).rounded())
        var head="近 \(minutes) 分钟 +\(tokens(added)) Token"
        if let rate=feed.rate(now),rate>0 {head+=" · \(tokens(rate))/分"}
        text(head,x:column,y:2,w:plot.width,size:8,color:.secondaryLabelColor,align:.right)
        let font=NSFont.monospacedDigitSystemFont(ofSize:8,weight:.medium)
        let badge="实时",badgeWidth=MenuLayout.width(badge,font:font)+10
        let pulse=0.55+0.45*CGFloat(0.5+0.5*sin(now.timeIntervalSince(born)*2*Double.pi/1.6))
        starAccent.withAlphaComponent(night ? 0.22 : 0.14).setFill();NSBezierPath(roundedRect:NSRect(x:0,y:1,width:badgeWidth+8,height:13),xRadius:6.5,yRadius:6.5).fill()
        starAccent.withAlphaComponent(pulse).setFill();NSBezierPath(ovalIn:NSRect(x:5,y:5.5,width:4,height:4)).fill()
        text(badge,x:12,y:2,w:badgeWidth,size:8,color:starAccent,weight:.medium)
        // Round marks at round values; as the scale eases to a new size they glide to their new heights.
        for (value,label) in [(0.0,"0"),(target/2,Self.compact(target/2)),(target,Self.compact(target))] {
            let f=CGFloat(min(1.1,value/ceiling)),y=plot.maxY-f*plot.height
            text(label,x:0,y:y-5,w:column-4,size:8,color:.secondaryLabelColor,numeric:true)
            line(NSPoint(x:plot.minX,y:y),NSPoint(x:plot.maxX,y:y),color:NSColor.separatorColor.withAlphaComponent(f==0 ? 0.3 : 0.14),width:0.5)
        }
        text(minutes<=1 ? "1 分钟前" : "\(minutes) 分钟前",x:plot.minX,y:plot.maxY+7,w:80,size:8.5,color:.secondaryLabelColor)
        text("现在",x:plot.maxX-48,y:plot.maxY+7,w:48,size:8.5,color:.secondaryLabelColor,align:.right)
        guard let base else {
            text("正在读取第一笔数据…",x:plot.minX,y:plot.midY-6,w:plot.width,size:9,color:.tertiaryLabelColor,align:.center);return
        }
        func x(_ time:Date)->CGFloat {plot.minX+CGFloat(time.timeIntervalSince(start)/span)*plot.width}
        func y(_ value:Double)->CGFloat {plot.maxY-CGFloat(min(1.08,max(0,value/ceiling)))*plot.height}
        // Steps: a line holds its value until the next reading, then rises to it over the reading's interval.
        func path(_ id:String?)->NSBezierPath {
            let curve=NSBezierPath();var previous=value(id,at:start,base:base)
            curve.move(to:NSPoint(x:plot.minX,y:y(previous)))
            for sample in points {
                let next=id.map{(sample.parts[$0] ?? 0)-(base.parts[$0] ?? 0)} ?? sample.total-base.total
                if next != previous {curve.line(to:NSPoint(x:x(sample.at)-6,y:y(previous)));curve.curve(to:NSPoint(x:x(sample.at),y:y(next)),controlPoint1:NSPoint(x:x(sample.at)-3,y:y(previous)),controlPoint2:NSPoint(x:x(sample.at)-3,y:y(next)))}
                previous=next
            }
            curve.line(to:NSPoint(x:plot.maxX,y:y(previous)))
            return curve
        }
        NSGraphicsContext.saveGraphicsState();NSBezierPath(rect:plot.insetBy(dx:0,dy:-6)).addClip()
        let totalPath=path(nil)
        let area=totalPath.copy() as! NSBezierPath;area.line(to:NSPoint(x:plot.maxX,y:plot.maxY));area.line(to:NSPoint(x:plot.minX,y:plot.maxY));area.close()
        NSGraphicsContext.saveGraphicsState();area.addClip()
        NSGradient(starting:total.withAlphaComponent(night ? 0.28 : 0.18),ending:total.withAlphaComponent(0))?.draw(from:NSPoint(x:plot.midX,y:plot.minY),to:NSPoint(x:plot.midX,y:plot.maxY),options:[])
        NSGraphicsContext.restoreGraphicsState()
        for id in order {let curve=path(id);curve.lineWidth=1.1;curve.lineJoinStyle = .round;(colors[id] ?? .secondaryLabelColor).withAlphaComponent(0.85).setStroke();curve.stroke()}
        totalPath.lineWidth=1.8;totalPath.lineJoinStyle = .round;totalPath.lineCapStyle = .round;total.setStroke();totalPath.stroke()
        NSGraphicsContext.restoreGraphicsState()
        // The newest reading lands with a ripple at the line's end.
        let end=NSPoint(x:plot.maxX,y:y(value(nil,at:now,base:base)))
        if let ripple,now.timeIntervalSince(ripple)<1.1,!NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            let t=CGFloat(now.timeIntervalSince(ripple)/1.1),radius=3+13*(1-pow(1-t,2))
            let ring=NSBezierPath(ovalIn:NSRect(x:end.x-radius,y:end.y-radius,width:radius*2,height:radius*2));ring.lineWidth=1.5
            total.withAlphaComponent(0.6*(1-t)).setStroke();ring.stroke()
        }
        total.withAlphaComponent(0.2).setFill();NSBezierPath(ovalIn:NSRect(x:end.x-5,y:end.y-5,width:10,height:10)).fill()
        total.setFill();NSBezierPath(ovalIn:NSRect(x:end.x-2.5,y:end.y-2.5,width:5,height:5)).fill()
    }
}
// MARK: - Detail pages
// What used to open beside the panel now opens inside it, as Token Monitor shows its details: the panel slides to the
// row's page, a row with more under it opens in place on an indented guide line, a usage reads as a coloured bar, and
// every row that does something sinks while pressed.
enum DetailStyle {
    static let indent:CGFloat=16,inset:CGFloat=8,lead:CGFloat=22
    static func titleFont(_ e:Entry,depth:Int)->NSFont {
        info(e) ? .systemFont(ofSize:10.5) : .systemFont(ofSize:depth==0 ? 12.5 : 12,weight:depth==0 && !e.children.isEmpty ? .medium : .regular)
    }
    static let valueFont=NSFont.monospacedDigitSystemFont(ofSize:11.5,weight:.medium)
    static let detailFont=NSFont.systemFont(ofSize:10.5)
    static let sectionFont=NSFont.systemFont(ofSize:10,weight:.semibold)
    // A plain fact (where data comes from, a note): quieter than the rows that can be acted on or carry a figure.
    static func info(_ e:Entry)->Bool {!e.actionable && e.value==nil && e.statusLight==nil && e.checked==nil && e.usageLevel==nil}
    // An on/off setting reads as a switch on the right, not a checkbox.
    static func toggle(_ e:Entry)->Bool {e.checked != nil && e.radio != true && (e.action?.last=="true" || e.action?.last=="false")}
    static func leads(_ e:Entry)->Bool {(e.checked != nil && !toggle(e)) || e.statusLight != nil || e.image != nil || e.symbol != nil}
    // Every action on a page carries an icon, so a level's text starts in one column whichever rows the engine drew
    // an icon for (it names only those its SwiftBar menu shows); the rest take one from what they do. A web link keeps
    // its small arrow on the right, so its icon on the left is a globe rather than a second arrow.
    static let actionSymbols:[String:String]=[
        "refresh-monitor":"arrow.clockwise","check-app-updates":"arrow.clockwise","retry-claude-usage":"arrow.clockwise",
        "choose-source":"folder","choose-client-home":"folder","reveal-source":"folder","reveal-client-home":"folder",
        "auto-source":"arrow.uturn.backward","reset-client-connection":"arrow.uturn.backward","set-proxy-address":"link",
        "choose-admin-token":"key","configure-connection":"key","test-api-key":"checkmark.shield","rename-api-key":"pencil",
        "remove-api-key":"trash","clear-connection":"trash","reset-modules":"arrow.counterclockwise","open-claude":"macwindow",
        "set-subscription":"checkmark.circle","set-claude-local":"dot.radiowaves.left.and.right","set-claude-live":"xmark.circle",
        "export-usage":"square.and.arrow.down","set-cny-rate":"arrow.left.arrow.right","detect-modules":"sparkle.magnifyingglass"]
    static func iconed(_ e:Entry)->Entry {
        guard e.separator != true,e.section != true,e.statusLight==nil else{return e}
        var e=e
        if e.href?.hasPrefix("http")==true,e.children.isEmpty {e.image=nil;e.symbol="globe";return e}
        guard e.image==nil,e.symbol==nil,e.children.isEmpty,e.checked==nil,e.statusLight==nil else{return e}
        if let href=e.href {e.symbol=href.hasPrefix("file:") ? "macwindow" : "globe";return e}
        guard let action=e.action?.first else{return e}
        e.symbol=action=="configure-connection" && e.label.contains("地址") ? "link" : actionSymbols[action]
        return e
    }
    // Something to do rather than something to read: tinted like a link, red when it takes something away.
    static func command(_ e:Entry)->Bool {e.children.isEmpty && (e.action != nil || e.href != nil) && e.value==nil && e.checked==nil && e.statusLight==nil}
    static func destructive(_ e:Entry)->Bool {
        ["remove-api-key","clear-connection","stop","reset-modules"].contains(e.action?.first ?? "") || e.label.hasPrefix("删除") || e.label.hasPrefix("清除")
    }
    // "已用 38%", "已用 本周 38% · Fable 39%": the tightest share, for the bar under a usage row.
    static func share(_ e:Entry)->Double? {
        guard e.usageLevel != nil,let value=e.value else{return nil}
        let pattern=try! NSRegularExpression(pattern:#"\d+(\.\d+)?(?=%)"#)
        let shares=pattern.matches(in:value,range:NSRange(value.startIndex...,in:value)).compactMap{Range($0.range,in:value).flatMap{Double(value[$0])}}
        return shares.max().map{min(1,max(0,$0/100))}
    }
    // Where even use would put the bar now: the share used, less how far ahead of even pace the detail says it is.
    static func pace(_ e:Entry)->Double? {
        guard let used=share(e),let detail=e.detail else{return nil}
        if detail.contains("与匀速持平") {return used}
        guard let range=detail.range(of:#"比匀速(多用|少用) \d+(\.\d+)?%"#,options:.regularExpression) else{return nil}
        let text=detail[range],ahead=text.contains("多用")
        guard let number=text.range(of:#"\d+(\.\d+)?"#,options:.regularExpression).flatMap({Double(text[$0])}) else{return nil}
        return min(1,max(0,used-(ahead ? number : -number)/100))
    }
    static func barColor(_ level:String?)->NSColor {level=="critical" ? .systemRed : level=="warning" ? .systemOrange : starAccent}
    static func left(depth:Int)->CGFloat {inset+CGFloat(depth)*indent}
    static func valueText(_ e:Entry,room:CGFloat)->String? {
        guard let value=e.value else{return nil}
        for form in [value]+(e.fallbacks ?? []) where MenuLayout.width(form,font:valueFont)<=room {return form}
        return e.fallbacks?.last ?? value
    }
    // aligned: a sibling has an icon, so every row of the level starts its text in the same column.
    static let switchSize=NSSize(width:30,height:18)
    static func layout(_ e:Entry,depth:Int,width:CGFloat,aligned:Bool)->(title:NSRect,value:NSRect?,valueText:String?,bar:NSRect?,detail:NSRect?,height:CGFloat) {
        let left=left(depth:depth)+(aligned || leads(e) ? lead : 0),right=width-inset-(e.children.isEmpty ? (e.href != nil ? 16 : 0) : 18)
        let valueText=toggle(e) ? nil : valueText(e,room:(right-left)*0.62)
        let valueWidth=toggle(e) ? switchSize.width : valueText.map{MenuLayout.width($0,font:valueFont)+(e.attention==true ? 14 : 0)} ?? 0
        let titleWidth=max(40,right-left-(valueWidth>0 ? valueWidth+10 : 0)),font=titleFont(e,depth:depth)
        let titleHeight=MenuLayout.height(e.label,font:font,width:titleWidth)
        var y:CGFloat=info(e) ? 5 : 8
        let title=NSRect(x:left,y:y,width:titleWidth,height:titleHeight);y+=titleHeight
        let value=valueWidth>0 ? NSRect(x:right-valueWidth,y:title.minY,width:valueWidth,height:max(titleHeight,16)) : nil
        var bar:NSRect?
        if share(e) != nil {y+=6;bar=NSRect(x:left,y:y,width:right-left,height:4);y+=4}
        var detail:NSRect?
        if let text=e.detail {let h=MenuLayout.height(text,font:detailFont,width:right-left);y+=3;detail=NSRect(x:left,y:y,width:right-left,height:h);y+=h}
        y+=info(e) ? 5 : 8
        return (title,value,valueText,bar,detail,max(e.actionable ? 32 : 22,ceil(y)))
    }
}

final class DetailRow:NSButton {
    override func acceptsFirstMouse(for event:NSEvent?)->Bool{true}
    let entry:Entry,depth:Int,aligned:Bool
    var open=false {didSet{if open != oldValue{turnChevron(animated:true)}}}
    var invoke:((DetailRow)->Void)?
    // The row's action is running: a spinner stands in its figure until the result arrives.
    var busy=false {didSet{if busy != oldValue{showBusy()}}}
    private var spinner:NSProgressIndicator?
    // A switch's knob, 0 off to 1 on; it slides as soon as the switch is clicked.
    private var knob:CGFloat=0;private var knobTimer:Timer?
    private var hovering=false;private var tracking:NSTrackingArea?;private var glyph:NSImage?;private var chevron:NSImageView?
    init(_ entry:Entry,depth:Int,aligned:Bool,frame:NSRect,open:Bool){
        self.entry=entry;self.depth=depth;self.aligned=aligned;self.open=open;super.init(frame:frame)
        title="";isBordered=false;setButtonType(.momentaryChange);target=self;action=#selector(pressed);isEnabled=entry.actionable
        setAccessibilityLabel(entry.label+(entry.value.map{", "+$0} ?? "")+(entry.detail.map{", "+$0} ?? ""))
        if let raw=entry.image,let data=Data(base64Encoded:raw),let img=NSImage(data:data){glyph=img;glyph?.isTemplate=entry.template ?? false}
        else if let symbol=entry.symbol{glyph=NSImage(systemSymbolName:symbol,accessibilityDescription:nil)}
        knob=entry.checked==true ? 1 : 0
        if !entry.children.isEmpty {
            let image=NSImageView(frame:NSRect(x:bounds.width-DetailStyle.inset-12,y:0,width:12,height:12))
            image.image=NSImage(systemSymbolName:"chevron.right",accessibilityDescription:nil)?.withSymbolConfiguration(.init(pointSize:10,weight:.semibold))
            image.contentTintColor = .tertiaryLabelColor;image.wantsLayer=true;addSubview(image);chevron=image
            setAccessibilityExpanded(open)
        }
        wantsLayer=true
    }
    required init?(coder:NSCoder){fatalError()}
    override var isFlipped:Bool{true}
    @objc private func pressed(){
        if DetailStyle.toggle(entry) {slideKnob(to:knob>0.5 ? 0 : 1)}
        invoke?(self)
    }
    private func slideKnob(to target:CGFloat){
        knobTimer?.invalidate()
        guard !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion else{knob=target;needsDisplay=true;return}
        let start=knob,began=Date()
        let timer=Timer(timeInterval:1.0/60,repeats:true){[weak self] timer in
            guard let self else{timer.invalidate();return}
            let t=min(1,Date().timeIntervalSince(began)/0.18);self.knob=start+(target-start)*CGFloat(1-pow(1-t,3));self.needsDisplay=true
            if t>=1{timer.invalidate()}
        }
        knobTimer=timer;RunLoop.main.add(timer,forMode:.common)
    }
    private func showBusy(){
        spinner?.removeFromSuperview();spinner=nil
        guard busy else{needsDisplay=true;return}
        let line=DetailStyle.layout(entry,depth:depth,width:bounds.width,aligned:aligned).title
        let spin=NSProgressIndicator(frame:NSRect(x:bounds.width-DetailStyle.inset-(entry.href != nil && entry.children.isEmpty ? 30 : 14),y:line.minY+(min(line.height,17)-14)/2,width:14,height:14))
        spin.style = .spinning;spin.controlSize = .small;spin.isIndeterminate=true;spin.startAnimation(nil);addSubview(spin);spinner=spin;needsDisplay=true
    }
    override func layout(){super.layout();placeChevron()}
    private func placeChevron(){
        guard let chevron else{return}
        let line=DetailStyle.layout(entry,depth:depth,width:bounds.width,aligned:aligned).title
        let mid=line.minY+min(line.height,ceil(DetailStyle.titleFont(entry,depth:depth).ascender-DetailStyle.titleFont(entry,depth:depth).descender))/2
        chevron.frameCenterRotation=0;chevron.frame=NSRect(x:bounds.width-DetailStyle.inset-12,y:mid-6,width:12,height:12);turnChevron(animated:false)
    }
    // Open, the chevron points down; it turns as the rows below slide out.
    private func turnChevron(animated:Bool){
        guard let chevron else{return}
        setAccessibilityExpanded(open)
        let angle:CGFloat=open ? 90 : 0
        if animated && !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            NSAnimationContext.runAnimationGroup{context in context.duration=0.22;context.allowsImplicitAnimation=true;chevron.animator().frameCenterRotation=angle}
        } else {chevron.frameCenterRotation=angle}
    }
    private func tone(_ e:Entry)->NSColor {
        DetailStyle.destructive(e) ? .systemRed : DetailStyle.command(e) ? starAccent : e.actionable ? .labelColor : .secondaryLabelColor
    }
    override func updateTrackingAreas(){super.updateTrackingAreas();if let tracking{removeTrackingArea(tracking)};tracking=NSTrackingArea(rect:bounds,options:[.mouseEnteredAndExited,.activeAlways,.inVisibleRect],owner:self,userInfo:nil);addTrackingArea(tracking!)}
    override func mouseEntered(with event:NSEvent){hovering=true;needsDisplay=true}
    override func mouseExited(with event:NSEvent){hovering=false;needsDisplay=true}
    override func mouseDown(with event:NSEvent){if isEnabled && !busy {Press.track(self,scale:0.985){super.mouseDown(with:event)}} else if !busy {super.mouseDown(with:event)}}
    override func viewDidChangeEffectiveAppearance(){super.viewDidChangeEffectiveAppearance();needsDisplay=true}
    override func draw(_ dirtyRect:NSRect){
        let e=entry,night=dark(self),layout=DetailStyle.layout(e,depth:depth,width:bounds.width,aligned:aligned)
        if isEnabled && (hovering || isHighlighted) {
            NSColor.labelColor.withAlphaComponent(isHighlighted ? (night ? 0.14 : 0.09) : (night ? 0.065 : 0.045)).setFill()
            NSBezierPath(roundedRect:bounds.insetBy(dx:2,dy:1),xRadius:8,yRadius:8).fill()
        }
        // Nested rows hang from a guide line under their parent's leading edge.
        for level in 0..<depth {
            let x=DetailStyle.left(depth:level)+6
            NSColor.labelColor.withAlphaComponent(night ? 0.24 : 0.16).setFill();NSRect(x:x,y:0,width:1,height:bounds.height).fill()
        }
        let titleFont=DetailStyle.titleFont(e,depth:depth),mid=layout.title.minY+ceil(titleFont.ascender-titleFont.descender)/2
        let leadX=DetailStyle.left(depth:depth)
        if let checked=e.checked,!DetailStyle.toggle(e) {
            let box=NSRect(x:leadX+1,y:mid-6.5,width:13,height:13),radio=e.radio==true
            let outline=NSBezierPath(roundedRect:box,xRadius:radio ? 6.5 : 3.5,yRadius:radio ? 6.5 : 3.5)
            if checked {
                starAccent.setFill();outline.fill();NSColor.white.setFill();NSColor.white.setStroke()
                if radio {NSBezierPath(ovalIn:box.insetBy(dx:4,dy:4)).fill()}
                else {let tick=NSBezierPath();tick.move(to:NSPoint(x:box.minX+3,y:box.midY));tick.line(to:NSPoint(x:box.minX+5.5,y:box.maxY-3.5));tick.line(to:NSPoint(x:box.maxX-3,y:box.minY+3.5));tick.lineWidth=1.6;tick.lineCapStyle = .round;tick.lineJoinStyle = .round;tick.stroke()}
            } else {NSColor.secondaryLabelColor.withAlphaComponent(0.55).setStroke();outline.lineWidth=1;outline.stroke()}
        } else if let state=e.statusLight {drawStatusLight(state,centre:NSPoint(x:leadX+7,y:mid),in:self)}
        else if let glyph {
            let rect=NSRect(x:leadX,y:mid-7.5,width:15,height:15)
            if glyph.isTemplate {
                let image=NSImage(size:rect.size);image.lockFocus();glyph.draw(in:NSRect(origin:.zero,size:rect.size))
                tone(e).set();NSRect(origin:.zero,size:rect.size).fill(using:.sourceIn);image.unlockFocus()
                image.draw(in:rect)
            } else {glyph.draw(in:rect)}
        }
        let titleColor:NSColor=DetailStyle.info(e) ? .secondaryLabelColor : DetailStyle.destructive(e) ? .systemRed : .labelColor
        MenuLayout.draw(e.label,in:layout.title,font:titleFont,color:busy ? titleColor.withAlphaComponent(0.55) : titleColor)
        if DetailStyle.toggle(e),let rect=layout.value {
            let track=NSRect(x:rect.maxX-DetailStyle.switchSize.width,y:mid-DetailStyle.switchSize.height/2,width:DetailStyle.switchSize.width,height:DetailStyle.switchSize.height)
            let off=NSColor.labelColor.withAlphaComponent(night ? 0.22 : 0.16),on=starAccent
            (off.usingColorSpace(.sRGB)?.blended(withFraction:knob,of:on.usingColorSpace(.sRGB) ?? on) ?? on).setFill()
            NSBezierPath(roundedRect:track,xRadius:track.height/2,yRadius:track.height/2).fill()
            let dot=NSRect(x:track.minX+2+(track.width-track.height)*knob,y:track.minY+2,width:track.height-4,height:track.height-4)
            NSGraphicsContext.saveGraphicsState()
            let shadow=NSShadow();shadow.shadowBlurRadius=2;shadow.shadowOffset=NSSize(width:0,height:-0.5);shadow.shadowColor=NSColor.black.withAlphaComponent(0.25);shadow.set()
            NSColor.white.setFill();NSBezierPath(ovalIn:dot).fill()
            NSGraphicsContext.restoreGraphicsState()
        } else if busy {}
        else if let rect=layout.value,let text=layout.valueText {
            let height=MenuLayout.height(text,font:DetailStyle.valueFont,width:rect.width+2),y=mid-height/2
            if e.attention==true {
                let pill=NSRect(x:rect.minX,y:mid-9,width:rect.width,height:18)
                starAccent.withAlphaComponent(night ? 0.22 : 0.12).setFill();NSBezierPath(roundedRect:pill,xRadius:9,yRadius:9).fill()
                MenuLayout.draw(text,in:NSRect(x:pill.minX,y:y,width:pill.width,height:height),font:DetailStyle.valueFont,color:night ? accent("opencodex",self) : starAccent,alignment:.center)
            } else if let configured=MenuLayout.configured(text) {
                drawStatusLight(configured ? "ok" : "unknown",centre:NSPoint(x:rect.minX+3.5,y:mid),in:self)
                MenuLayout.draw(text,in:NSRect(x:rect.minX+12,y:y,width:rect.width-10,height:height),font:DetailStyle.valueFont,color:.secondaryLabelColor,alignment:.right)
            } else {
                let color=usageColor(e.usageLevel) ?? (e.actionable || e.usageLevel != nil ? NSColor.labelColor : NSColor.secondaryLabelColor)
                MenuLayout.draw(text,in:NSRect(x:rect.minX-2,y:y,width:rect.width+2,height:height),font:DetailStyle.valueFont,color:color,alignment:.right)
            }
        }
        if let bar=layout.bar,let share=DetailStyle.share(e) {
            NSColor.labelColor.withAlphaComponent(night ? 0.12 : 0.08).setFill();NSBezierPath(roundedRect:bar,xRadius:2,yRadius:2).fill()
            DetailStyle.barColor(e.usageLevel).setFill()
            NSBezierPath(roundedRect:NSRect(x:bar.minX,y:bar.minY,width:max(4,bar.width*CGFloat(share)),height:bar.height),xRadius:2,yRadius:2).fill()
            // Even pace: where the bar would reach by now if the window were used evenly to its reset.
            if let pace=DetailStyle.pace(e) {
                let x=min(bar.maxX-1,max(bar.minX+1,bar.minX+bar.width*CGFloat(pace)))
                NSColor.labelColor.withAlphaComponent(night ? 0.75 : 0.6).setFill()
                NSBezierPath(roundedRect:NSRect(x:x-1,y:bar.minY-3,width:2,height:bar.height+6),xRadius:1,yRadius:1).fill()
            }
        }
        if let rect=layout.detail,let text=e.detail {MenuLayout.draw(text,in:rect,font:DetailStyle.detailFont,color:.secondaryLabelColor)}
        if e.children.isEmpty,e.href != nil,let arrow=NSImage(systemSymbolName:"arrow.up.right",accessibilityDescription:nil)?.withSymbolConfiguration(.init(pointSize:9,weight:.semibold)) {
            let rect=NSRect(x:bounds.width-DetailStyle.inset-10,y:mid-5,width:10,height:10)
            let tinted=NSImage(size:rect.size);tinted.lockFocus();arrow.draw(in:NSRect(origin:.zero,size:rect.size));NSColor.tertiaryLabelColor.set();NSRect(origin:.zero,size:rect.size).fill(using:.sourceIn);tinted.unlockFocus()
            tinted.draw(in:rect)
        }
    }
}

// A few short choices side by side, as Token Monitor's DAY · MONTH · TOTAL: the chosen one sits on a raised block that
// slides to the next choice the moment it is clicked.
final class DetailSegments:NSView {
    override func acceptsFirstMouse(for event:NSEvent?)->Bool{true}
    let entries:[Entry];var invoke:((Entry)->Void)?
    private var chosen:Int?;private var hovered:Int?;private let block=NSView();private var labels:[NSTextField]=[];private var area:NSTrackingArea?
    static let height:CGFloat=28,font=NSFont.systemFont(ofSize:11.5),chosenFont=NSFont.systemFont(ofSize:11.5,weight:.semibold)
    static func fits(_ entries:[Entry],width:CGFloat)->Bool {
        let each=(width-4)/CGFloat(entries.count)
        return entries.count<=4 && entries.allSatisfy{MenuLayout.width($0.label,font:chosenFont)+14<=each}
    }
    init(_ entries:[Entry],frame:NSRect){
        self.entries=entries;chosen=entries.firstIndex{$0.checked==true};super.init(frame:frame)
        wantsLayer=true;block.wantsLayer=true;block.layer?.cornerRadius=6;addSubview(block)
        for entry in entries {
            let label=NSTextField(labelWithString:entry.label);label.alignment = .center;label.lineBreakMode = .byTruncatingTail;addSubview(label);labels.append(label)
        }
        setAccessibilityElement(true);setAccessibilityRole(.radioGroup)
        arrange(animated:false)
    }
    required init?(coder:NSCoder){fatalError()}
    override var isFlipped:Bool{true}
    private func slot(_ index:Int)->NSRect {
        let each=(bounds.width-4)/CGFloat(entries.count)
        return NSRect(x:2+each*CGFloat(index),y:2,width:each,height:bounds.height-4)
    }
    private func arrange(animated:Bool){
        for (index,label) in labels.enumerated() {
            let on=index==chosen;label.font=on ? Self.chosenFont : Self.font;label.textColor=on ? .labelColor : .secondaryLabelColor
            let height=ceil(label.intrinsicContentSize.height);label.frame=slot(index).insetBy(dx:4,dy:0);label.frame.origin.y=(bounds.height-height)/2;label.frame.size.height=height
        }
        block.isHidden=chosen==nil
        guard let chosen else{return}
        let target=slot(chosen)
        if animated && !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            NSAnimationContext.runAnimationGroup{context in context.duration=0.22;context.timingFunction=App.pageCurve;context.allowsImplicitAnimation=true;block.animator().frame=target}
        } else {block.frame=target}
    }
    override func viewDidChangeEffectiveAppearance(){super.viewDidChangeEffectiveAppearance();needsDisplay=true}
    override func draw(_ dirtyRect:NSRect){
        let night=dark(self)
        NSColor.labelColor.withAlphaComponent(night ? 0.07 : 0.05).setFill();NSBezierPath(roundedRect:bounds,xRadius:8,yRadius:8).fill()
        if let hovered,hovered != chosen {NSColor.labelColor.withAlphaComponent(night ? 0.06 : 0.04).setFill();NSBezierPath(roundedRect:slot(hovered),xRadius:6,yRadius:6).fill()}
        block.layer?.backgroundColor=(night ? NSColor(calibratedWhite:1,alpha:0.16) : NSColor.white).cgColor
        block.layer?.shadowColor=NSColor.black.cgColor;block.layer?.shadowOpacity=night ? 0 : 0.12;block.layer?.shadowRadius=1.5;block.layer?.shadowOffset=CGSize(width:0,height:-0.5)
    }
    override func updateTrackingAreas(){super.updateTrackingAreas();if let area{removeTrackingArea(area)};area=NSTrackingArea(rect:bounds,options:[.mouseMoved,.mouseEnteredAndExited,.activeAlways,.inVisibleRect],owner:self,userInfo:nil);addTrackingArea(area!)}
    private func index(_ event:NSEvent)->Int? {let point=convert(event.locationInWindow,from:nil);return entries.indices.first{slot($0).contains(point)}}
    override func mouseMoved(with event:NSEvent){let next=index(event);if next != hovered{hovered=next;needsDisplay=true}}
    override func mouseExited(with event:NSEvent){hovered=nil;needsDisplay=true}
    override func mouseDown(with event:NSEvent){
        guard let index=index(event) else{return}
        Press.set(labels[index],true,scale:0.94)
    }
    override func mouseUp(with event:NSEvent){
        for label in labels {Press.set(label,false,scale:0.94)}
        guard let index=index(event),index != chosen,entries[index].action != nil else{return}
        chosen=index;arrange(animated:true);needsDisplay=true;invoke?(entries[index])
    }
}

// A page's rows. Opening or closing one moves the rows below to their new places, fades the new ones in, and
// tells the panel its new height.
final class DetailView:FlippedView {
    private enum Item {case row(Entry,Int,Bool),section(Entry,Int),rule(Int),segments([Entry],Int)}
    private let entries:[Entry],busy:[String]?
    private(set) var open:Set<String>
    var invoke:((Entry)->Void)?
    // A switch or a choice was set: the page already shows it, so the panel need not redraw at once.
    var chose:((Entry)->Void)?
    var resized:((CGFloat,Bool)->Void)?
    var toggled:((Set<String>)->Void)?
    private var views:[String:NSView]=[:]
    init(_ entries:[Entry],width:CGFloat,open:Set<String>,busy:[String]?=nil){
        self.entries=entries;self.open=open;self.busy=busy
        super.init(frame:NSRect(x:0,y:0,width:width,height:0))
        place(animated:false)
    }
    required init?(coder:NSCoder){fatalError()}
    private func items()->[(String,Item)] {
        var out:[(String,Item)]=[]
        func walk(_ list:[Entry],_ depth:Int,_ parent:String){
            let list=list.map(DetailStyle.iconed)
            var seen:[String:Int]=[:];let aligned=list.contains(where:DetailStyle.leads)
            // A run of short choices becomes one segmented control, where the run stood.
            var runs:[Int:Int]=[:],start=0
            while start<list.count {
                var end=start
                while end<list.count && list[end].radio==true && list[end].children.isEmpty {end+=1}
                if end-start>=2 && DetailSegments.fits(Array(list[start..<end]),width:bounds.width-DetailStyle.left(depth:depth)-DetailStyle.inset) {runs[start]=end}
                start=max(end,start+1)
            }
            var skip=Set<Int>()
            for (index,entry) in list.enumerated() where !skip.contains(index) {
                if let end=runs[index] {
                    for i in index..<end {skip.insert(i)}
                    out.append((parent+"/§\(index)",.segments(Array(list[index..<end]),depth)));continue
                }
                if entry.separator==true {
                    // A rule between groups, never at the top or bottom of a level.
                    if index>0 && index<list.count-1 {out.append((parent+"/—\(index)",.rule(depth)))};continue
                }
                let base=parent+"/"+entry.key,count=seen[base,default:0];seen[base]=count+1
                let id=count==0 ? base : base+"#\(count)"
                if entry.section==true {out.append((id,.section(entry,depth)));continue}
                out.append((id,.row(entry,depth,aligned)))
                if !entry.children.isEmpty && open.contains(id) {walk(entry.children,depth+1,id)}
            }
        }
        walk(entries,0,"")
        return out
    }
    static func height(_ entries:[Entry],width:CGFloat,open:Set<String>)->CGFloat {DetailView(entries,width:width,open:open).frame.height}
    // A page with a single row to open (one Key, one session) arrives with it open: there is nothing to choose between.
    static func opening(_ page:Entry)->Set<String> {
        let openable=page.children.filter{!$0.children.isEmpty && $0.separator != true && $0.section != true}
        return openable.count==1 ? ["/"+openable[0].key] : []
    }
    private func size(_ item:Item)->CGFloat {
        switch item {
        case .row(let e,let depth,let aligned):return DetailStyle.layout(e,depth:depth,width:bounds.width,aligned:aligned).height
        case .section:return 28
        case .rule:return 13
        case .segments:return DetailSegments.height+12
        }
    }
    private func make(_ id:String,_ item:Item,_ frame:NSRect)->NSView {
        switch item {
        case .row(let e,let depth,let aligned):
            let row=DetailRow(e,depth:depth,aligned:aligned,frame:frame,open:open.contains(id))
            row.invoke={[weak self] row in self?.pressed(id,row)}
            if let busy,e.action==busy,!DetailStyle.toggle(e) {row.busy=true}
            return row
        case .section(let e,let depth):
            let label=NSTextField(labelWithString:e.label);label.font=DetailStyle.sectionFont;label.textColor = .secondaryLabelColor
            label.frame=frame.insetBy(dx:0,dy:0);label.frame.origin.x=DetailStyle.left(depth:depth);label.frame.origin.y=frame.minY+11;label.frame.size.height=15
            let box=FlippedView(frame:frame);label.frame.origin.y=11;box.addSubview(label);return box
        case .rule(let depth):
            let box=FlippedView(frame:frame),line=NSBox(frame:NSRect(x:DetailStyle.left(depth:depth),y:6,width:frame.width-DetailStyle.left(depth:depth)-DetailStyle.inset,height:1));line.boxType = .separator;box.addSubview(line);return box
        case .segments(let list,let depth):
            let box=FlippedView(frame:frame),left=DetailStyle.left(depth:depth)
            for level in 0..<depth {let guide=NSView(frame:NSRect(x:DetailStyle.left(depth:level)+6,y:0,width:1,height:frame.height));guide.wantsLayer=true;guide.layer?.backgroundColor=NSColor.labelColor.withAlphaComponent(0.2).cgColor;box.addSubview(guide)}
            let control=DetailSegments(list,frame:NSRect(x:left,y:6,width:frame.width-left-DetailStyle.inset,height:DetailSegments.height))
            control.invoke={[weak self] entry in self?.chose?(entry)};box.addSubview(control);return box
        }
    }
    private func pressed(_ id:String,_ row:DetailRow){
        if DetailStyle.toggle(row.entry) {chose?(row.entry);return}
        guard !row.entry.children.isEmpty else{invoke?(row.entry);return}
        if open.contains(id){open=open.filter{$0 != id && !$0.hasPrefix(id+"/")}}else{open.insert(id)}
        row.open=open.contains(id);toggled?(open);place(animated:true)
    }
    private func place(animated:Bool){
        let list=items(),motion=animated && !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        var y:CGFloat=0,keep=Set<String>(),fresh:[NSView]=[],moves:[(NSView,NSRect)]=[]
        for (id,item) in list {
            let frame=NSRect(x:0,y:y,width:bounds.width,height:size(item));y+=frame.height;keep.insert(id)
            if let view=views[id] {moves.append((view,frame))}
            else {
                let view=make(id,item,frame);views[id]=view;addSubview(view)
                if motion {view.alphaValue=0;view.frame=frame.offsetBy(dx:0,dy:-6);moves.append((view,frame));fresh.append(view)}
            }
        }
        let gone=views.filter{!keep.contains($0.key)}
        for key in gone.keys {views[key]=nil}
        let height=ceil(y)
        if motion {
            if height>frame.height {setFrameSize(NSSize(width:bounds.width,height:height))}
            resized?(height,true)
            NSAnimationContext.runAnimationGroup({context in
                context.duration=0.26;context.timingFunction=CAMediaTimingFunction(controlPoints:0.22,1,0.36,1);context.allowsImplicitAnimation=true
                for (view,frame) in moves {view.animator().frame=frame}
                for view in fresh {view.animator().alphaValue=1}
                for view in gone.values {view.animator().alphaValue=0}
            },completionHandler:{[weak self] in
                for view in gone.values {view.removeFromSuperview()}
                if let self,self.frame.height != height {self.setFrameSize(NSSize(width:self.bounds.width,height:height))}
            })
        } else {
            for (view,frame) in moves {view.frame=frame}
            for view in gone.values {view.removeFromSuperview()}
            setFrameSize(NSSize(width:bounds.width,height:height))
            resized?(height,false)
        }
    }
}
struct Flyout {var panel:Panel;var key:String;weak var row:Row?}
// Presentation is optimistic, but a saved preference revision wins over an older data refresh.
struct PresentationPreferences:Decodable {
    var revision:Int64;var visible:[String];var appearance:String;var theme:String?;var allowCollapse:Bool;var expandedProviders:[String]
}
struct ActionReceipt:Decodable {var id:String;var status:String;var preferences:PresentationPreferences?}
struct PanelInteraction {
    enum Submission {case start,queued,ignored}
    var source:Snapshot?;var confirmed:PresentationPreferences?;var active:[String]?;var queued:[[String]]=[]
    static func isPreference(_ action:[String])->Bool {
        guard let name=action.first else{return false}
        return ["set-module","set-appearance","set-theme","reset-modules","set-collapse","set-expanded"].contains(name)
    }
    // Actions without a dialog keep the panel open so their result appears in place.
    static func keepsPanelOpen(_ action:[String])->Bool {
        guard let name=action.first else{return false}
        // A setting that changes without a question stays on screen, so the change is seen where it was made.
        if quietSettings.contains(name) {return true}
        // These ask before turning on, in a dialog the panel would cover; turning them off asks nothing.
        if ["set-cursor-usage","set-claude-oauth","set-claude-local","set-claude-live"].contains(name) && action.last=="false" {return true}
        if name=="set-keep-usage" && action.last=="true" {return true}
        return ["refresh-view","refresh-source","refresh","refresh-live","refresh-monitor","check-app-updates","set-subscription","set-range","set-group","test-api-key","retry-claude-usage"].contains(name) || isPreference(action)
    }
    static let quietSettings:Set<String>=["set-currency","set-service-status","set-pin-interval","set-menu-bar","set-usage-alert","set-reset-reminder","set-session-alerts"]
    // A switch or a choice shows its new state the moment it is clicked; the saved state confirms it after.
    static let choices:Set<String>=quietSettings.union(["set-cursor-usage","set-claude-oauth","set-claude-local","set-claude-live","set-keep-usage","set-subscription"])
    static func applyChoice(_ action:[String],to value:inout Snapshot){
        guard let name=action.first,choices.contains(name) else{return}
        func walk(_ list:inout [Entry])->Bool {
            if let index=list.firstIndex(where:{$0.action==action}) {
                if list[index].radio==true {for i in list.indices where list[i].radio==true {list[i].checked = i==index}}
                else if list[index].checked != nil,let last=action.last,last=="true" || last=="false" {
                    list[index].checked = last=="true";list[index].action?[action.count-1] = last=="true" ? "false" : "true"
                }
                return true
            }
            for i in list.indices where walk(&list[i].children) {return true}
            return false
        }
        if !walk(&value.entries),var options=value.moduleOptions {_=walk(&options);value.moduleOptions=options}
    }
    static func automatic(_ action:[String])->Bool {
        ["refresh-view","refresh-source","refresh-live"].contains(action.first ?? "")
    }
    // SwiftBar's name for a plugin is its file name up to the first dot. SwiftBar 2.1.0/2.1.1
    // match a swiftbar:// link only by that name or the full path, never by the whole file name.
    static func swiftBarName(_ path:String)->String {
        String(URL(fileURLWithPath:path).lastPathComponent.split(separator:".",omittingEmptySubsequences:false).first ?? "")
    }
    static func mayExpire(_ action:[String],claimed:Bool)->Bool {
        // SwiftBar can miss a wake-up. A claimed user action can legitimately wait for a dialog.
        automatic(action) || isPreference(action) || !claimed
    }
    // While an action runs, only the row that started it says so.
    static func runningLabel(_ entry:Entry,active:[String]?)->String? {
        active?.first=="test-api-key" && entry.action==active ? "正在测试…" : nil
    }
    mutating func submit(_ action:[String])->Submission {
        guard !action.isEmpty else{return .ignored}
        guard active != nil else{active=action;return .start}
        guard !Self.automatic(action),queued.count<64 else{return .ignored}
        if !Self.isPreference(action) && (action==active || queued.contains(action)){return .ignored}
        queued.append(action);return .queued
    }
    mutating func finish(_ preferences:PresentationPreferences?) {
        if let preferences,preferences.revision >= (confirmed?.revision ?? -1){confirmed=preferences}
        active=nil
    }
    mutating func next()->[String]? {
        guard active==nil,!queued.isEmpty else{return nil}
        let action=queued.removeFirst();active=action;return action
    }
    func displayed()->Snapshot? {
        guard var value=source else{return nil}
        var authoritative=value.preferences
        if let confirmed,confirmed.revision > (authoritative?.revision ?? -1){authoritative=confirmed}
        if let authoritative{Self.applySettings(authoritative,to:&value)}
        for action in (active.map{[$0]} ?? [])+queued{Self.applyPreference(action,to:&value);Self.applyChoice(action,to:&value)}
        return value
    }
    static func applySettings(_ settings:PresentationPreferences,to value:inout Snapshot) {
        value.preferences=settings;value.appearance=settings.appearance
        value.dashboard?.allowCollapse=settings.allowCollapse;value.dashboard?.expandedProviders=settings.expandedProviders
        value.dashboard?.showSummary=settings.visible.contains("summary")
        value.dashboard?.providers.removeAll{!settings.visible.contains($0.id)}
        if let options=value.moduleOptions {
            for option in options where option.action?.first=="set-module" {
                guard let id=option.action?.dropFirst().first else{continue}
                applyPreference(["set-module",id,settings.visible.contains(id) ? "true" : "false"],to:&value)
            }
        }
        syncCollapseOption(settings.allowCollapse,to:&value)
        applyPreference(["set-appearance",settings.appearance],to:&value)
        applyPreference(["set-theme",settings.theme ?? "system"],to:&value)
    }
    static func syncCollapseOption(_ enabled:Bool,to value:inout Snapshot){
        value.dashboard?.allowCollapse=enabled
        for i in value.moduleOptions?.indices ?? 0..<0 where value.moduleOptions?[i].action?.first=="set-collapse" {
            value.moduleOptions?[i].checked=enabled;value.moduleOptions?[i].action=["set-collapse",enabled ? "false" : "true"]
        }
    }
    static func applyPreference(_ action:[String],to value:inout Snapshot){
        guard let kind=action.first else{return}
        if kind=="set-module",action.count==3 {
            for i in value.moduleOptions?.indices ?? 0..<0 where value.moduleOptions?[i].action?.prefix(2).elementsEqual(action.prefix(2)) == true {
                let enabled=action[2]=="true";value.moduleOptions?[i].checked=enabled
                value.moduleOptions?[i].action=["set-module",action[1],enabled ? "false" : "true"]
            }
        }
        if kind=="set-collapse",action.count==2 {
            let enabled=action[1]=="true"
            // Turning folding on starts with every card folded.
            if enabled{value.dashboard?.expandedProviders=[]}
            syncCollapseOption(enabled,to:&value)
        }
        if kind=="set-expanded",action.count==3 {
            var expanded=Set(value.dashboard?.expandedProviders ?? [])
            if action[2]=="true"{expanded.insert(action[1])}else{expanded.remove(action[1])}
            value.dashboard?.expandedProviders=expanded.sorted()
        }
        if ["set-appearance","set-theme"].contains(kind),action.count==2 {
            if kind=="set-appearance"{value.appearance=action[1]}else{value.theme=action[1]}
            func update(_ entries:inout [Entry]){
                for i in entries.indices {
                    if entries[i].action?.first==kind {entries[i].checked=entries[i].action?.last==action[1]}
                    update(&entries[i].children)
                }
            }
            if var entries=value.moduleOptions{update(&entries);value.moduleOptions=entries}
        }
    }

}
// Atomic JSON publication replaces files, so observe the containing directory.
// A closed panel has no polling timer and no global pointer monitors.
final class SnapshotObserver {
    private var source:DispatchSourceFileSystemObject?
    private var debounce:DispatchWorkItem?
    var active:Bool {source != nil}
    func start(directory:String,changed:@escaping ()->Void) {
        stop()
        let fd=open(directory,O_EVTONLY)
        guard fd>=0 else{return}
        let watcher=DispatchSource.makeFileSystemObjectSource(fileDescriptor:fd,eventMask:[.write,.delete,.rename,.revoke],queue:.main)
        watcher.setCancelHandler{close(fd)}
        watcher.setEventHandler{[weak self] in
            guard let self else{return}
            if let events=self.source?.data,!events.intersection([.delete,.rename,.revoke]).isEmpty {self.stop();changed();return}
            self.debounce?.cancel()
            let task=DispatchWorkItem(block:changed);self.debounce=task
            DispatchQueue.main.asyncAfter(deadline:.now()+0.08,execute:task)
        }
        source=watcher;watcher.resume()
    }
    func stop(){debounce?.cancel();debounce=nil;source?.cancel();source=nil}
    deinit{stop()}
}

// Submenus switch a fixed delay after the pointer enters a row, as native menus do;
// further movement inside the same row keeps the countdown instead of restarting it.
final class HoverIntent {
    var delay:TimeInterval=0.08
    private(set) var target:String?
    private var task:DispatchWorkItem?
    func schedule(_ key:String,after wait:TimeInterval?=nil,_ action:@escaping ()->Void){
        if target==key{return}
        cancel();target=key
        let item=DispatchWorkItem{[weak self] in self?.target=nil;self?.task=nil;action()}
        task=item;DispatchQueue.main.asyncAfter(deadline:.now()+(wait ?? delay),execute:item)
    }
    func cancel(){task?.cancel();task=nil;target=nil}
}

// The standalone app uses the same snapshots and action receipts as the SwiftBar helper.
final class MacHost {
    private let worker:URL
    private let refreshQueue=DispatchQueue(label:"starmeter.refresh",qos:.utility)
    private let actionQueue=DispatchQueue(label:"starmeter.actions",qos:.userInitiated)
    private var running=false
    private var refreshAgain=false
    var snapshotPath:String?
    init?(bundle:Bundle) {
        guard let url=bundle.url(forResource:"starmeter-worker",withExtension:"sh") else{return nil}
        worker=url
    }
    @discardableResult private func run(_ job:String, output:Pipe?=nil) -> Bool {
        let process=Process();process.executableURL=worker;process.arguments=[job]
        process.standardOutput=output as Any? ?? FileHandle.nullDevice
        process.standardError=FileHandle.nullDevice
        do{try process.run();process.waitUntilExit();return process.terminationStatus==0}catch{return false}
    }
    func prepare(_ ready:@escaping (String?)->Void) {
        refreshQueue.async {
            let output=Pipe()
            guard self.run("paths",output:output),
                  let data=try? output.fileHandleForReading.readToEnd(),
                  let payload=(try? JSONSerialization.jsonObject(with:data)) as? [String:String],
                  let path=payload["snapshot"] else{DispatchQueue.main.async{ready(nil)};return}
            let ok=self.run("prepare")
            DispatchQueue.main.async{self.snapshotPath=ok ? path : nil;ready(self.snapshotPath)}
        }
    }
    func refresh() {
        if running{refreshAgain=true;return}
        running=true
        refreshQueue.async {
            self.run("refresh")
            DispatchQueue.main.async {
                self.running=false
                if self.refreshAgain{self.refreshAgain=false;self.refresh()}
            }
        }
    }
    func action() {
        actionQueue.async {
            self.run("action")
            DispatchQueue.main.async{self.refresh()}
        }
    }
    private(set) var notificationsAllowed=false
    private var askedForNotifications=false
    // On launch the app asks once, so usage and balance alerts work without a trip to the menu; once the
    // user has answered, the system keeps that answer and the request never shows again.
    func checkNotifications(askFirst:Bool=false) {
        UNUserNotificationCenter.current().getNotificationSettings { [weak self] settings in
            DispatchQueue.main.async {
                guard let self else{return}
                if askFirst,settings.authorizationStatus == .notDetermined,!self.askedForNotifications {
                    self.askedForNotifications=true
                    UNUserNotificationCenter.current().requestAuthorization(options:[.alert,.sound]){[weak self] _,_ in
                        DispatchQueue.main.async{self?.checkNotifications()}
                    }
                    return
                }
                guard let path=self.snapshotPath else{return}
                let enabled=settings.authorizationStatus == .authorized || settings.authorizationStatus == .provisional
                self.notificationsAllowed=enabled
                let url=URL(fileURLWithPath:path).deletingLastPathComponent().appendingPathComponent("notifications-enabled.json")
                let data=Data((enabled ? "{\"schema\":1,\"allowed\":true}" : "{\"schema\":1,\"allowed\":false}").utf8)
                if (try? Data(contentsOf:url)) != data{try? data.write(to:url,options:.atomic)}
                self.deliverNotices()
            }
        }
    }
    func deliverNotices() {
        guard let path=snapshotPath else{return}
        let directory=URL(fileURLWithPath:path).deletingLastPathComponent()
        for url in (try? FileManager.default.contentsOfDirectory(at:directory,includingPropertiesForKeys:nil)) ?? [] where url.lastPathComponent.hasPrefix("notice-") && url.pathExtension=="json" {
            defer{try? FileManager.default.removeItem(at:url)}
            guard let data=try? Data(contentsOf:url),let notice=(try? JSONSerialization.jsonObject(with:data)) as? [String:String],
                  let body=notice["body"] else{continue}
            let content=UNMutableNotificationContent();content.title="Starmeter";content.subtitle=notice["subtitle"] ?? "";content.body=body
            UNUserNotificationCenter.current().add(UNNotificationRequest(identifier:UUID().uuidString,content:content,trigger:nil))
        }
    }
}

final class App:NSObject,NSApplicationDelegate,NSMenuDelegate,NSSharingServicePickerDelegate {
    private let panel=Panel(contentRect:.zero,styleMask:[.borderless,.nonactivatingPanel],backing:.buffered,defer:false)
    private var rangePicker:NSPopUpButton?;private var anchoredMenuOpen=false;private var toast:(text:String,until:Date,ok:Bool)?;private var toastView:NSView?;private weak var shareButton:NSButton?;private weak var detectButton:NSButton?;private weak var settingsButton:NSButton?;private var settingsFromHome=false;private var settingsReturn:CGFloat=0;private var hintTask:DispatchWorkItem?;private var hintView:NSView?;private var userApp:NSRunningApplication?;private var firstActivation=true;private var managerOnly=false {didSet{if managerOnly && !oldValue{leadingModules=nil}}}
    // The ticked modules when the list opened: they lead their sections until it opens again.
    private var leadingModules:Set<String>?
    private var snapshot:Snapshot?;private var snapshotPath="";private var pluginPath="";private var modified:Date?
    private var anchor:NSPoint = .zero;private var anchorRect:NSRect = .zero;private var scrollOffset:CGFloat=0
    private var fallbackTimer:Timer?;private let observer=SnapshotObserver();private var actionTimeout:DispatchWorkItem?;private var reloadTask:DispatchWorkItem?;private var monitors:[Any]=[];private var pendingAction:String?
    private var interaction=PanelInteraction()
    private var macHost:MacHost?
    private var statusItem:NSStatusItem?
    private var refreshTimer:Timer?
    private var preparing=false
    private var requestedHome:Bool?
    private var sourceChecks:[String:Date]=[:]
    private var flyouts:[Flyout]=[];private var rows:[[String:Row]]=[];private let hoverIntent=HoverIntent();private var closeTask:DispatchWorkItem?;private var closeLevel=0
    private var trail:[(point:NSPoint,time:TimeInterval)]=[]
    private func setup(_ window:Panel){window.backgroundColor = .clear;window.isOpaque=false;window.hasShadow=true;window.level = .popUpMenu;window.hidesOnDeactivate=false;window.isReleasedWhenClosed=false;window.acceptsMouseMovedEvents=true;window.collectionBehavior=[.canJoinAllSpaces,.fullScreenAuxiliary,.transient,.ignoresCycle];window.escape={[weak self] in self?.back()}}
    func applicationDidFinishLaunching(_ notification:Notification){
        let args=CommandLine.arguments
        if Bundle.main.object(forInfoDictionaryKey:"TokenMonitorStandalone") as? Bool == true {
            guard let host=MacHost(bundle:.main) else{NSApp.terminate(nil);return}
            macHost=host
            let item=NSStatusBar.system.statusItem(withLength:NSStatusItem.squareLength)
            statusItem=item
            if let button=item.button {
                // The logo's one-colour mark; its dots fall on whole pixels at 18 points.
                let mark=Bundle.main.url(forResource:"starmeter-mark",withExtension:"svg",subdirectory:"assets").flatMap{NSImage(contentsOf:$0)}
                mark?.size=NSSize(width:18,height:18)
                button.image=mark ?? NSImage(systemSymbolName:"sparkles",accessibilityDescription:"Starmeter")
                button.image?.isTemplate=true;button.image?.accessibilityDescription="Starmeter"
                button.toolTip="Starmeter"
                button.target=self;button.action=#selector(starPressed(_:))
                button.sendAction(on:[.leftMouseUp,.rightMouseUp])
            }
            preparing=true
            requestedHome=args.contains("--show-panel") ? false : nil
            host.prepare { [weak self, weak host] path in
                guard let self,let host else{return};self.preparing=false
                guard let path else {
                    if self.requestedHome != nil{self.showActionFailure("无法初始化本机数据，请重新打开 Starmeter。")}
                    return
                }
                self.snapshotPath=path;self.observeSnapshot();self.updateStatusTitle();host.checkNotifications(askFirst:true);host.refresh()
                // Pinned, the live tick already re-reads every couple of seconds; this minute's read would only repeat it.
                self.refreshTimer=Timer.scheduledTimer(withTimeInterval:60,repeats:true){[weak self,weak host] _ in if self?.pinned != true{host?.refresh()}}
                self.refreshTimer?.tolerance=12
                if let modules=self.requestedHome{self.requestedHome=nil;self.managerOnly=modules;self.show()}
            }
        }else {
            guard let si=args.firstIndex(of:"--snapshot"),si+1<args.count,let pi=args.firstIndex(of:"--plugin"),pi+1<args.count else{NSApp.terminate(nil);return}
            snapshotPath=args[si+1];pluginPath=args[pi+1]
            observeSnapshot()
        }
        setup(panel);panel.setAccessibilityLabel("AI 用量面板")
        NSWorkspace.shared.notificationCenter.addObserver(self,selector:#selector(accessibilityChanged),name:NSWorkspace.accessibilityDisplayOptionsDidChangeNotification,object:nil)
        if let front=NSWorkspace.shared.frontmostApplication,front != NSRunningApplication.current{userApp=front}
        NSWorkspace.shared.notificationCenter.addObserver(forName:NSWorkspace.didActivateApplicationNotification,object:nil,queue:.main){[weak self] note in
            if let app=note.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication,app != NSRunningApplication.current{self?.userApp=app}
        }
        NotificationCenter.default.addObserver(self,selector:#selector(rangeTrackingBegan(_:)),name:NSMenu.didBeginTrackingNotification,object:nil)
        NotificationCenter.default.addObserver(self,selector:#selector(rangeTrackingEnded(_:)),name:NSMenu.didEndTrackingNotification,object:nil)
        if macHost==nil{show()}
    }
    @objc private func starPressed(_ button:NSStatusBarButton) {
        let event=NSApp.currentEvent
        if event?.type == .rightMouseUp || event?.modifierFlags.contains(.option)==true || event?.modifierFlags.contains(.control)==true {
            if panel.isVisible{hide(returningFocus:false)}
            let menu=NSMenu()
            func add(_ title:String,_ action:Selector)->NSMenuItem {
                let item=NSMenuItem(title:title,action:action,keyEquivalent:"");item.target=self;menu.addItem(item);return item
            }
            _=add("主页模块…",#selector(openStandaloneModules))
            _=add("自动检测…",#selector(standaloneDetect))
            menu.addItem(.separator())
            let login=add("登录时启动",#selector(toggleLogin));login.state=SMAppService.mainApp.status == .enabled ? .on : .off
            _=add(macHost?.notificationsAllowed == true ? "通知设置…" : "开启系统通知…",#selector(allowNotices))
            _=add("关于 Starmeter",#selector(standaloneAbout))
            menu.addItem(.separator())
            _=add("退出 Starmeter",#selector(quitStandalone))
            menu.popUp(positioning:nil,at:NSPoint(x:0,y:button.bounds.minY),in:button)
        }else if panel.isVisible{hide()}else{openStandalone(modules:false)}
    }
    private func openStandalone(modules:Bool) {
        if snapshotPath.isEmpty {
            if preparing{requestedHome=modules}else{showActionFailure("无法读取本机数据，请退出后重新打开 Starmeter。")}
            return
        }
        managerOnly=modules;macHost?.checkNotifications();show()
    }
    @objc private func openStandaloneModules(){openStandalone(modules:true)}
    @objc private func standaloneDetect(){if !snapshotPath.isEmpty{performAction(["detect-modules"])}}
    @objc private func toggleLogin(){
        do{
            if SMAppService.mainApp.status == .enabled {try SMAppService.mainApp.unregister()}
            else{try SMAppService.mainApp.register()}
        }catch{showActionFailure("请将 Starmeter 拖到「应用程序」后重试，或检查系统设置中的登录项。")}
    }
    // Not asked yet: ask. Answered: the system shows no second request, so open this app's notification settings.
    @objc private func allowNotices(){
        UNUserNotificationCenter.current().getNotificationSettings { [weak self] settings in
            DispatchQueue.main.async {
                if settings.authorizationStatus == .notDetermined {
                    UNUserNotificationCenter.current().requestAuthorization(options:[.alert,.sound]){[weak self] _,_ in
                        DispatchQueue.main.async{self?.macHost?.checkNotifications()}
                    }
                }else if let url=URL(string:"x-apple.systempreferences:com.apple.preference.notifications?id="+(Bundle.main.bundleIdentifier ?? "")) {
                    NSWorkspace.shared.open(url)
                }
            }
        }
    }
    @objc private func standaloneAbout(){
        let version=Bundle.main.object(forInfoDictionaryKey:"CFBundleShortVersionString") as? String ?? ""
        let alert=NSAlert();alert.messageText="Starmeter "+version;alert.informativeText="macOS 独立版 · minicoke"
        alert.addButton(withTitle:"好");alert.addButton(withTitle:"发布与更新")
        activateForPanel()
        if alert.runModal() == NSApplication.ModalResponse.alertSecondButtonReturn,let url=URL(string:"https://github.com/timecatbit/starmeter/releases"){NSWorkspace.shared.open(url)}else{returnFocus()}
    }
    @objc private func quitStandalone(){NSApp.terminate(nil)}
    private func observeSnapshot(){
        guard !observer.active else{return}
        observer.start(directory:URL(fileURLWithPath:snapshotPath).deletingLastPathComponent().path){[weak self] in
            guard let self else{return};self.macHost?.deliverNotices();self.checkAction();self.updateFallback();self.updateStatusTitle();if self.panel.isVisible{self.reload()}
        }
        updateFallback()
    }
    private func installPointerMonitors(){
        guard monitors.isEmpty else{return}
        if let monitor=NSEvent.addGlobalMonitorForEvents(matching:[.leftMouseDown,.rightMouseDown],handler:{[weak self] event in
            // The system share sheet draws in another process, so its clicks arrive here as outside clicks.
            // A pinned panel stays where it is whatever is clicked; the star or Esc closes it.
            guard let self,!self.anchoredMenuOpen,!self.pinned else{return}
            // SwiftBar routes right-click and Option-click to its native menu.
            // Only an ordinary left-click on the star retains the panel toggle.
            if event.type == .rightMouseDown || event.modifierFlags.contains(.option) || !self.anchorRect.contains(NSEvent.mouseLocation){self.hide(returningFocus:false)}
        }){monitors.append(monitor)}
        if let monitor=NSEvent.addGlobalMonitorForEvents(matching:.mouseMoved,handler:{[weak self] _ in self?.checkPointer()}){monitors.append(monitor)}
        if let monitor=NSEvent.addLocalMonitorForEvents(matching:.mouseMoved,handler:{[weak self] event in self?.checkPointer();return event}){monitors.append(monitor)}
        if let monitor=NSEvent.addLocalMonitorForEvents(matching:.rightMouseDown,handler:{[weak self] event in
            guard let self,let window=event.window,(window === self.panel || self.flyouts.contains(where:{$0.panel === window})),let view=window.contentView else{return event}
            self.showModuleMenu(event,in:view);return nil
        }){monitors.append(monitor)}
    }
    private func removePointerMonitors(){for monitor in monitors{NSEvent.removeMonitor(monitor)};monitors=[]}
    private func publishVisibility(_ visible:Bool){
        let url=URL(fileURLWithPath:snapshotPath).deletingLastPathComponent().appendingPathComponent("panel-state.json")
        if let data=try? JSONSerialization.data(withJSONObject:["schema":1,"visible":visible,"pid":ProcessInfo.processInfo.processIdentifier]) {
            try? data.write(to:url,options:.atomic);try? FileManager.default.setAttributes([.posixPermissions:0o600],ofItemAtPath:url.path)
        }
        updateFallback()
    }
    // The figure the user chose for beside the star (今天的 API 估算 or 最紧张的订阅额度), read from each new snapshot.
    private func updateStatusTitle(){
        guard macHost != nil,let item=statusItem,let button=item.button,!snapshotPath.isEmpty,
              let data=try? Data(contentsOf:URL(fileURLWithPath:snapshotPath)),
              let value=try? JSONSerialization.jsonObject(with:data) as? [String:Any] else{return}
        let text=(value["menuBarText"] as? String) ?? ""
        guard button.title != text else{return}
        item.length=text.isEmpty ? NSStatusItem.squareLength : NSStatusItem.variableLength
        button.font=NSFont.monospacedDigitSystemFont(ofSize:NSFont.systemFontSize,weight:.regular)
        button.title=text;button.imagePosition=text.isEmpty ? .imageOnly : .imageLeading
    }
    // Pinned: the panel ignores clicks elsewhere, floats above ordinary windows instead of menus, and asks for a
    // live refresh every few seconds (2 by default, set in 外观与设置). A tick is dropped while any action is running.
    private var pinned=false;private var liveTimer:Timer?;private weak var pinButton:HintButton?
    private var liveFeed:LiveFeed?;private var liveChart:LiveChart?
    // The readings on screen, so the next ones roll from them; and when the last live tick went out.
    private var shownDashboard:Dashboard?;private var liveTick=Date()
    // The page on screen: empty for the homepage (or the module grid), else the keys of the row opened, and which of
    // that page's rows are open in place. slide is the next layout's direction: 1 in to a page, -1 back, 0 none.
    private var pagePath:[String]=[];private var pageOpen:Set<String>=[];private var slide:CGFloat=0;private var homeScroll:CGFloat=0
    private weak var pageScroll:ContentScrollView?
    // A switch or segment just moved on the page: the redraw that confirms it waits until its slide has finished.
    private var quietUntil=Date.distantPast;private var quietLayout:DispatchWorkItem?
    @objc private func togglePin(){hideHint();setPinned(!pinned);if pinned{performAction(["refresh-live"])}}
    private func setPinned(_ value:Bool){
        guard value != pinned else{return}
        pinned=value;panel.level=value ? .floating : .popUpMenu;showPin();scheduleLive()
        // Pinned, the trend gives way to the live chart; unpinned, it comes back.
        if value {
            let feed=LiveFeed();liveFeed=feed;liveChart=LiveChart(feed:feed)
            if let dashboard=snapshot?.dashboard{feed.add(dashboard,at:modified ?? Date())}
        } else {liveFeed=nil;liveChart?.removeFromSuperview();liveChart=nil}
        if panel.isVisible && pagePath.isEmpty {layout()}
    }
    private func showPin(){
        guard let pin=pinButton else{return}
        let interval=Int(snapshot?.pinInterval ?? 2)
        pin.image=NSImage(systemSymbolName:pinned ? "pin.fill" : "pin",accessibilityDescription:nil)?.withSymbolConfiguration(.init(pointSize:11,weight:.semibold))
        pin.hint=pinned ? "取消钉住" : "钉住，每 \(interval) 秒刷新";pin.setAccessibilityLabel(pinned ? "取消钉住面板" : "钉住面板")
        showLiveRing(pin)
    }
    // Pinned, the pin's outline fills over each interval and starts again as the next reading is asked for:
    // the one timer a pinned panel runs, made visible.
    private func showLiveRing(_ pin:NSButton){
        pin.wantsLayer=true
        pin.layer?.sublayers?.filter{$0.name=="live-ring"}.forEach{$0.removeFromSuperlayer()}
        guard pinned,let layer=pin.layer else{return}
        let interval=max(1,snapshot?.pinInterval ?? 2),ring=CAShapeLayer();ring.name="live-ring";ring.frame=pin.bounds
        ring.path=CGPath(roundedRect:pin.bounds.insetBy(dx:0.75,dy:0.75),cornerWidth:7,cornerHeight:7,transform:nil)
        ring.fillColor=nil;pin.effectiveAppearance.performAsCurrentDrawingAppearance{ring.strokeColor=starAccent.cgColor};ring.lineWidth=1.5;ring.lineCap = .round
        layer.addSublayer(ring)
        guard !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion else{return}
        let sweep=CABasicAnimation(keyPath:"strokeEnd");sweep.fromValue=0;sweep.toValue=1;sweep.duration=interval;sweep.repeatCount = .infinity
        sweep.timeOffset=Date().timeIntervalSince(liveTick).truncatingRemainder(dividingBy:interval);ring.add(sweep,forKey:"sweep")
    }
    private func scheduleLive(){
        liveTimer?.invalidate();liveTimer=nil
        guard pinned else{return}
        let interval=max(1,snapshot?.pinInterval ?? 2)
        liveTick=Date()
        liveTimer=Timer.scheduledTimer(withTimeInterval:interval,repeats:true){[weak self] _ in
            guard let self,self.pinned,self.panel.isVisible,!self.anchoredMenuOpen else{return}
            self.liveTick=Date();if let pin=self.pinButton{self.showLiveRing(pin)}
            self.performAction(["refresh-live"])
        };liveTimer?.tolerance=interval*0.05
    }
    private func updateFallback(){
        fallbackTimer?.invalidate();fallbackTimer=nil
        guard !observer.active,(panel.isVisible || pendingAction != nil) else{return}
        fallbackTimer=Timer.scheduledTimer(withTimeInterval:1,repeats:true){[weak self] _ in
            guard let self else{return};self.observeSnapshot();self.checkAction();if self.panel.isVisible{self.reload()}
        };fallbackTimer?.tolerance=0.25
    }
    func applicationWillTerminate(_ notification:Notification){if !snapshotPath.isEmpty{publishVisibility(false)};observer.stop();removePointerMonitors();fallbackTimer?.invalidate();refreshTimer?.invalidate()}
    @objc private func accessibilityChanged(){if panel.isVisible{reload(force:true)}}
    private func showModuleMenu(_ event:NSEvent,in view:NSView){
        rangePicker?.menu?.cancelTracking();anchoredMenuOpen=false;hoverIntent.cancel();closeTask?.cancel();closeFlyouts(from:0)
        managerOnly=true;scrollOffset=0;layout()
    }
    // The gear on the homepage opens the module list in the same panel; 完成, Esc or a swipe goes back to the homepage.
    // Opened from the star's right-click menu, the list is the whole panel and 完成 closes it.
    @objc private func openSettings(){
        hideHint();closeFlyouts(from:0)
        if let scroll=(panel.contentView as? Surface)?.content.subviews.first as? NSScrollView {settingsReturn=scroll.contentView.bounds.origin.y}
        managerOnly=true;settingsFromHome=true;pagePath=[];pageOpen=[];scrollOffset=0;slide=1;layout();slide=0
    }
    @objc private func finishModules(){
        guard settingsFromHome else{hide();return}
        hideHint();settingsFromHome=false;managerOnly=false;pagePath=[];pageOpen=[];scrollOffset=settingsReturn;slide = -1;layout();slide=0
    }
    private func requestedMode()->Bool? {
        let request=URL(fileURLWithPath:snapshotPath).deletingLastPathComponent().appendingPathComponent("panel-open.json")
        guard let data=try? Data(contentsOf:request),let value=try? JSONSerialization.jsonObject(with:data) as? [String:Any],let created=value["createdAt"] as? Double,Date().timeIntervalSince1970-created<30 else{return nil}
        try? FileManager.default.removeItem(at:request)
        return value["mode"] as? String=="modules"
    }
    // Opening the app (the star) activates it. On launch that comes before the panel has been built, so
    // the first activation is left alone; later, if it lands after the panel closed, focus goes straight
    // back. A short wait tells a reopen that shows the panel from one that hides it. A dialog the app is
    // showing (About, a failure notice) keeps the focus it was opened with.
    func applicationDidBecomeActive(_ notification:Notification){
        if firstActivation{firstActivation=false;return}
        DispatchQueue.main.asyncAfter(deadline:.now()+0.6){[weak self] in guard let self,!self.panel.isVisible,NSApp.modalWindow==nil else{return};self.returnFocus()}
    }
    func applicationShouldHandleReopen(_ sender:NSApplication,hasVisibleWindows flag:Bool)->Bool{if macHost != nil{openStandalone(modules:false);return false};let requested=requestedMode();if panel.isVisible && (requested == nil || requested==managerOnly){hide()}else{if let requested{managerOnly=requested};show()};return false}
    private func nativeMenuVisible()->Bool {
        if macHost != nil{return false}
        let windows=CGWindowListCopyWindowInfo([.optionOnScreenOnly,.excludeDesktopElements],kCGNullWindowID) as? [[String:Any]] ?? []
        return windows.contains {row in
            guard row[kCGWindowOwnerName as String] as? String == "SwiftBar",
                  let layer=row[kCGWindowLayer as String] as? Int,layer==NSWindow.Level.popUpMenu.rawValue,
                  let bounds=row[kCGWindowBounds as String] as? [String:Double] else{return false}
            return (bounds["Width"] ?? 0)>80 && (bounds["Height"] ?? 0)>30
        }
    }
    private func show(){
        settingsFromHome=false
        if macHost==nil,let requested=requestedMode(){managerOnly=requested}
        leadingModules=nil
        guard !nativeMenuVisible() else{hide();return}
        scrollOffset=0;pagePath=[];pageOpen=[];slide=0
        if let button=statusItem?.button,let window=button.window {
            anchorRect=window.convertToScreen(button.convert(button.bounds,to:nil))
            anchor=NSPoint(x:anchorRect.midX,y:anchorRect.midY)
        }else {
            anchor=NSEvent.mouseLocation
            let screen=NSScreen.screens.first(where:{$0.frame.contains(anchor)}) ?? NSScreen.main!
            anchorRect=NSRect(x:anchor.x-24,y:screen.visibleFrame.maxY,width:48,height:screen.frame.maxY-screen.visibleFrame.maxY+8)
        }
        reload(force:true);panel.makeKeyAndOrderFront(nil);activateForPanel();observeSnapshot();installPointerMonitors();publishVisibility(true)
        if let macHost{macHost.refresh()}else if !managerOnly{performAction(["refresh-view"])}
    }

    // Glass and controls draw their inactive, greyer style until the app is active, so the panel used to
    // change colour on the first click. Opening the panel activates it; closing hands focus back.
    private func activateForPanel(){
        guard !NSApp.isActive else{return}
        if #available(macOS 14.0,*){NSApp.activate()}else{NSApp.activate(ignoringOtherApps:true)}
    }
    // Deactivating alone leaves no app focused, so hand focus to the app the user was in. Not when the
    // panel closed because they clicked another app or opened a web page: that app is coming forward.
    private func returnFocus(){
        guard NSApp.isActive,let app=userApp,!app.isTerminated else{return}
        if #available(macOS 14.0,*){NSApp.yieldActivation(to:app);app.activate()}else{app.activate(options:[])}
    }
    private func hide(returningFocus:Bool=true){setPinned(false);if settingsFromHome{settingsFromHome=false;managerOnly=false};shownDashboard=nil;pagePath=[];pageOpen=[];slide=0;if returningFocus{returnFocus()};hintTask?.cancel();hintView?.removeFromSuperview();hintView=nil;rangePicker?.menu?.cancelTracking();hoverIntent.cancel();closeTask?.cancel();closeFlyouts(from:0);panel.orderOut(nil);removePointerMonitors();reloadTask?.cancel();publishVisibility(false)}
    private func updatePresentation(){snapshot=interaction.displayed();layout()}
    private func reload(force:Bool=false){
        // Do not replace a button while the user is pressing it.
        guard !anchoredMenuOpen else{return}
        let moving = !flyouts.isEmpty && ProcessInfo.processInfo.systemUptime-(trail.last?.time ?? 0)<0.3
        if NSEvent.pressedMouseButtons & 1 != 0 || moving {
            reloadTask?.cancel();let task=DispatchWorkItem{[weak self] in self?.reload(force:force)};reloadTask=task
            DispatchQueue.main.asyncAfter(deadline:.now()+0.08,execute:task);return
        }
        let date=(try? FileManager.default.attributesOfItem(atPath:snapshotPath)[.modificationDate]) as? Date
        guard force || date != modified else{return}
        guard let data=try? Data(contentsOf:URL(fileURLWithPath:snapshotPath)),let value=try? JSONDecoder().decode(Snapshot.self,from:data) else{return}
        modified=date;interaction.source=value
        if pinned,let dashboard=value.dashboard{liveFeed?.add(dashboard,at:date ?? Date())}
        updatePresentation()
    }
    private func fillGrid(_ entries:[Entry],in content:NSView,y start:CGFloat,width:CGFloat)->CGFloat {
        rows=[[:]];var y=start;let cell=ModuleGrid.cellWidth(width)
        for section in ModuleGrid.sections(entries,leading:leadingModules) {
            if let title=section.title {
                let label=NSTextField(labelWithString:title);label.font = .systemFont(ofSize:12.5,weight:.semibold);label.textColor = .labelColor
                label.frame=NSRect(x:8,y:y+12,width:width-16,height:17);content.addSubview(label);y+=ModuleGrid.title
            }
            for line in ModuleGrid.rows(section) {
                let height=ModuleGrid.rowHeight(line)
                for (column,original) in line.enumerated() {
                    var entry=original;if let label=PanelInteraction.runningLabel(entry,active:interaction.active){entry.label=label}
                    let row=Row(entry,frame:NSRect(x:CGFloat(column)*(cell+ModuleGrid.spacing),y:y,width:cell,height:height));row.carded=true;row.columnInset=ModuleGrid.text
                    wire(row,depth:0);content.addSubview(row);rows[0][entry.key]=row
                }
                y+=height
            }
            y+=ModuleGrid.gap
        }
        return y
    }
    private func wire(_ row:Row,depth:Int){
        row.pointerAllowed={[weak self,weak row] in guard let self,let row else{return false};return self.pointerHits(row)}
        row.invoke={[weak self] e,r in self?.activate(e,row:r,depth:depth)};row.hover={[weak self] e,r in self?.hover(e,row:r,depth:depth)}
    }
    private func fillRows(_ entries:[Entry],in content:NSView,y start:CGFloat,width:CGFloat,depth:Int)->CGFloat {
        while rows.count<=depth{rows.append([:])};rows[depth]=[:];var y=start
        for original in entries {var entry=original;if let label=PanelInteraction.runningLabel(entry,active:interaction.active){entry.label=label}
            let left=MenuLayout.left(entry,entries);entry=MenuLayout.fitted(entry,width:width,left:left);let h=MenuLayout.rowHeight(entry,width:width,left:left)
            if entry.separator==true {content.addSubview(depth==0 ? Divider(frame:NSRect(x:8,y:y+5,width:width-16,height:2)) : {let line=NSBox(frame:NSRect(x:8,y:y+5,width:width-16,height:1));line.boxType = .separator;return line}())}
            else {let row=Row(entry,frame:NSRect(x:0,y:y,width:width,height:h));row.columnInset=entry.section==true ? 12 : left;wire(row,depth:depth);content.addSubview(row);rows[depth][entry.key]=row}
            y+=h
        }
        return y
    }
    // 浅色/深色 pins every window of the app (panel, sidebars, share pictures); 跟随系统 hands it back to macOS.
    private func applyTheme(_ theme:String?){
        let name:NSAppearance.Name?=["light":NSAppearance.Name.aqua,"dark":.darkAqua][theme ?? ""]
        guard NSApp.appearance?.name != name else{return}
        NSApp.appearance=name.flatMap{NSAppearance(named:$0)}
    }
    private func layout(){
        guard !anchoredMenuOpen,let snapshot else{return};applyTheme(snapshot.theme)
        // A page whose row has gone (a module turned off, a key removed) gives way to the homepage.
        if !pagePath.isEmpty && pageEntry()==nil {pagePath=[];pageOpen=[]}
        let previous=slide != 0 && panel.isVisible ? (panel.contentView as? Surface).flatMap(picture) : nil
        if slide==0,!pagePath.isEmpty,Date()<quietUntil {
            quietLayout?.cancel();let task=DispatchWorkItem{[weak self] in self?.layout()};quietLayout=task
            DispatchQueue.main.asyncAfter(deadline:.now()+quietUntil.timeIntervalSinceNow+0.02,execute:task);return
        }
        if let page=pageEntry() {layoutPage(page,previous:previous);return}
        if panel.isVisible,slide==0,let old=(panel.contentView as? Surface)?.content.subviews.first as? NSScrollView,old !== pageScroll {scrollOffset=old.contentView.bounds.origin.y}
        let opened=flyouts.map{$0.key};closeFlyouts(from:0);rows=[]
        let screen=NSScreen.screens.first(where:{$0.frame.contains(anchor)}) ?? NSScreen.main!,width:CGFloat=managerOnly ? ModuleGrid.width : 360,inner=width-16
        let entries=managerOnly ? snapshot.moduleOptions ?? [] : snapshot.entries
        let top:CGFloat=managerOnly ? 49 : (snapshot.dashboard.map{DashboardView.height($0)+8} ?? 36)
        if managerOnly && leadingModules==nil {leadingModules=Set(entries.filter{$0.isModule && $0.checked==true}.map{$0.key})}
        let body=managerOnly ? ModuleGrid.height(entries,leading:leadingModules) : entries.reduce(CGFloat(0)){$0+MenuLayout.rowHeight($1,width:inner,left:MenuLayout.left($1,entries))},contentHeight=top+body
        let height=min(contentHeight+16,screen.visibleFrame.height-16)
        let surface=Surface(frame:NSRect(x:0,y:0,width:width,height:height),glass:self.snapshot?.appearance != "solid")
        let scroll=ContentScrollView(frame:NSRect(x:8,y:8,width:inner,height:height-16));scroll.drawsBackground=false;scroll.hasVerticalScroller=contentHeight>scroll.frame.height;scroll.scrollerStyle = .overlay
        if managerOnly && settingsFromHome {scroll.swipedBack={[weak self] in self?.finishModules()}}
        let content=managerOnly ? BlankClickView(frame:NSRect(x:0,y:0,width:inner,height:contentHeight)) : FlippedView(frame:NSRect(x:0,y:0,width:inner,height:contentHeight))
        (content as? BlankClickView)?.clicked={[weak self] in self?.closeFlyouts(from:0)}
        if managerOnly {
            // From the homepage's gear the list is a page like the others, with the same way back; from the star's
            // right-click menu it is the whole panel and 完成 closes it.
            let lead:CGFloat=settingsFromHome ? 36 : 12
            if settingsFromHome {
                let back=HintButton(frame:NSRect(x:4,y:9,width:27,height:25))
                back.bare=true;back.controlSize = .small;back.title="";back.imagePosition = .imageOnly;back.contentTintColor = .labelColor
                back.image=NSImage(systemSymbolName:"chevron.left",accessibilityDescription:"返回")?.withSymbolConfiguration(.init(pointSize:11,weight:.semibold))
                back.hint="返回";back.hovered={[weak self] in self?.hoverHint($0,$1)};back.setAccessibilityLabel("返回")
                back.target=self;back.action=#selector(finishModules);content.addSubview(back)
            }
            let heading=NSTextField(labelWithString:"主页模块");heading.font = .systemFont(ofSize:14,weight:.semibold);heading.frame=NSRect(x:lead,y:7,width:inner-85,height:20);content.addSubview(heading)
            let count=entries.filter{$0.action?.first=="set-module" && $0.checked==true}.count
            let hint=NSTextField(labelWithString:"已启用 \(count) 项");hint.font = .systemFont(ofSize:10);hint.textColor = .secondaryLabelColor;hint.frame=NSRect(x:lead,y:29,width:inner-24,height:15);content.addSubview(hint)
            if !settingsFromHome {let done=NSButton(title:"完成",target:self,action:#selector(finishModules));done.bezelStyle = .rounded;done.controlSize = .small;done.frame=NSRect(x:inner-65,y:8,width:53,height:25);content.addSubview(done)}
        } else if let dashboard=snapshot.dashboard{let view=DashboardView(dashboard,width:inner)
            if pinned,let stamp=modified {let clock=DateFormatter();clock.dateFormat="HH:mm:ss";view.liveStamp="实时 "+clock.string(from:stamp)}
            if let previous=shownDashboard{view.animate(from:previous)};shownDashboard=dashboard;view.live=liveChart;view.toggleProvider={[weak self] id,expanded in self?.performAction(["set-expanded",id,expanded ? "true" : "false"])};view.chooseGroup={[weak self] key in self?.performAction(["set-group",key])};content.addSubview(view)}else{let label=NSTextField(labelWithString:"AI 用量面板");label.frame=NSRect(x:12,y:10,width:300,height:20);content.addSubview(label)}
        if managerOnly {_=fillGrid(entries,in:content,y:top,width:inner)}
        else {content.addSubview(Divider(frame:NSRect(x:8,y:top-6,width:inner-16,height:2)));_=fillRows(entries,in:content,y:top,width:inner,depth:0)}
        if !managerOnly,let dashboard=snapshot.dashboard,let choices=snapshot.rangeOptions {
            let picker=NSPopUpButton(frame:NSRect(x:inner-113,y:7.5,width:101,height:25),pullsDown:true)
            picker.controlSize = .small;picker.font = .systemFont(ofSize:11);picker.bezelStyle = .rounded
            picker.preferredEdge = .maxY;picker.setAccessibilityLabel("统计范围 · "+dashboard.rangeLabel)
            let menu=NSMenu(title:"统计范围");menu.autoenablesItems=false;menu.delegate=self
            let title=NSMenuItem(title:dashboard.rangeLabel,action:nil,keyEquivalent:"");menu.addItem(title)
            for choice in choices {
                let option=NSMenuItem(title:choice.label,action:#selector(chooseRange(_:)),keyEquivalent:"")
                option.target=self;option.representedObject=choice.action;option.state=choice.checked==true ? .on : .off
                option.isEnabled = true;menu.addItem(option)
            }
            picker.menu=menu;menu.delegate=self;rangePicker=picker;content.addSubview(picker)
            // Only the snapshot marks a range. Left to itself, the button also checks the clicked option,
            // which showed two checks while the previous range stays on screen.
            (picker.cell as? NSPopUpButtonCell)?.altersStateOfSelectedItem=false
            // Refresh leaves the button and current data in place; duplicate actions are coalesced.
            let refresh=HintButton(frame:NSRect(x:inner-143,y:7.5,width:27,height:25))
            refresh.bare=true;refresh.controlSize = .small;refresh.title=""
            refresh.image=NSImage(systemSymbolName:"arrow.clockwise",accessibilityDescription:"刷新数据")?.withSymbolConfiguration(.init(pointSize:11,weight:.semibold));refresh.imagePosition = .imageOnly
            refresh.contentTintColor = .labelColor;refresh.hint="刷新数据";refresh.hovered={[weak self] in self?.hoverHint($0,$1)};refresh.setAccessibilityLabel("刷新数据")
            refresh.target=self;refresh.action=#selector(refreshData)
            let share=HintButton(frame:NSRect(x:inner-173,y:7.5,width:27,height:25))
            share.bare=true;share.controlSize = .small;share.title=""
            share.image=NSImage(systemSymbolName:"square.and.arrow.up",accessibilityDescription:"分享用量图")?.withSymbolConfiguration(.init(pointSize:11,weight:.semibold));share.imagePosition = .imageOnly
            share.contentTintColor = .labelColor;share.hint="分享用量图";share.hovered={[weak self] in self?.hoverHint($0,$1)};share.setAccessibilityLabel("分享用量图")
            share.target=self;share.action=#selector(shareDashboard(_:));shareButton=share
            // Auto-detect is a rarely used setup action, so it is a plain icon like its neighbours; the
            // hover hint and accessibility label name it, and it always asks before changing anything.
            let detect=HintButton(frame:NSRect(x:inner-203,y:7.5,width:27,height:25))
            detect.bare=true;detect.controlSize = .small;detect.title=""
            detect.image=NSImage(systemSymbolName:"sparkle.magnifyingglass",accessibilityDescription:"自动检测")?.withSymbolConfiguration(.init(pointSize:11,weight:.semibold));detect.imagePosition = .imageOnly
            detect.contentTintColor = .labelColor;detect.setAccessibilityLabel("自动检测")
            detect.hint="自动检测";detect.hovered={[weak self] in self?.hoverHint($0,$1)}
            detect.target=self;detect.action=#selector(detectModules)
            // Pinned, the panel stays open and re-reads the range on screen every few seconds. Same button as
            // its neighbours; the filled pin alone says it is on.
            let pin=HintButton(frame:.zero)
            pin.bare=true;pin.controlSize = .small;pin.title="";pin.imagePosition = .imageOnly;pin.contentTintColor = .labelColor
            pin.hovered={[weak self] in self?.hoverHint($0,$1)};pin.target=self;pin.action=#selector(togglePin)
            pinButton=pin
            // Settings: the module list and 外观与设置, a click away instead of only behind the star's right-click menu.
            let settings=HintButton(frame:.zero)
            settings.bare=true;settings.controlSize = .small;settings.title="";settings.imagePosition = .imageOnly;settings.contentTintColor = .labelColor
            settings.image=NSImage(systemSymbolName:"gearshape",accessibilityDescription:"主页模块与设置")?.withSymbolConfiguration(.init(pointSize:11,weight:.semibold))
            settings.hint="主页模块与设置";settings.hovered={[weak self] in self?.hoverHint($0,$1)};settings.setAccessibilityLabel("主页模块与设置")
            settings.target=self;settings.action=#selector(openSettings)
            // Leftward from the range: refresh, share, auto-detect, the pin, the gear, 2pt apart. A long heading
            // (Summary hidden) keeps its room by leaving auto-detect out; it is still in 外观与设置 and the star's menu.
            let headingRight=12+ceil(MenuLayout.width(DashboardView.heading(dashboard),font:.systemFont(ofSize:14,weight:.semibold)))+6
            let fitsAll=inner-142-4*29>=headingRight
            for (index,button) in ([refresh,share]+(fitsAll ? [detect] : [])+[pin,settings]).enumerated() {
                button.frame=NSRect(x:inner-142-CGFloat(index)*29,y:7.5,width:27,height:25);content.addSubview(button)
            }
            detectButton=fitsAll ? detect : nil;settingsButton=settings;showPin()
            if pinned,liveTimer?.timeInterval != (snapshot.pinInterval ?? 2){scheduleLive()}
        }
        scroll.documentView=content;scroll.configureScrolling();scroll.contentView.scroll(to:NSPoint(x:0,y:min(scrollOffset,max(0,contentHeight-scroll.frame.height))));surface.content.addSubview(scroll);panel.contentView=surface
        let x=min(max(anchor.x-22,screen.visibleFrame.minX+4),screen.visibleFrame.maxX-width-4)
        place(NSRect(x:x,y:screen.visibleFrame.maxY-height-4,width:width,height:height),surface:surface,previous:previous);placeToast()
        for (depth,key) in opened.enumerated(){guard rows.indices.contains(depth),let row=rows[depth][key] else{break};openFlyout(row.entry,row:row,depth:depth,checkSource:false)}
        DispatchQueue.main.async{[weak self] in self?.checkPointer()}
    }
    private func rowRect(_ row:Row)->NSRect{guard let window=row.window else{return .zero};return window.convertToScreen(row.convert(row.bounds,to:nil))}

    // MARK: Pages
    private func pageEntry()->Entry? {
        var list=(managerOnly ? snapshot?.moduleOptions : snapshot?.entries) ?? [],found:Entry?
        for key in pagePath {
            guard let entry=list.first(where:{$0.key==key && !$0.children.isEmpty}) else{return nil}
            found=entry;list=entry.children
        }
        return found
    }
    private func push(_ entry:Entry){
        if let action=entry.onOpenAction,action.count==2 {
            let now=Date()
            if now.timeIntervalSince(sourceChecks[action[1]] ?? .distantPast)>=5 {sourceChecks[action[1]]=now;DispatchQueue.main.async{[weak self] in self?.performAction(action)}}
        }
        if let scroll=(panel.contentView as? Surface)?.content.subviews.first as? NSScrollView {homeScroll=scroll.contentView.bounds.origin.y}
        hideHint();pagePath=[entry.key];pageOpen=DetailView.opening(entry);slide=1;layout();slide=0
    }
    private func back(){
        guard !pagePath.isEmpty else{if settingsFromHome && managerOnly {finishModules()} else {hide()};return}
        hideHint();pagePath=[];pageOpen=[];scrollOffset=homeScroll;slide = -1;layout();slide=0
    }
    // The surface as it looks now, to slide away while the next page slides in.
    private func picture(_ view:NSView)->NSImage? {
        guard view.bounds.width>0,let rep=view.bitmapImageRepForCachingDisplay(in:view.bounds) else{return nil}
        view.cacheDisplay(in:view.bounds,to:rep);let image=NSImage(size:view.bounds.size);image.addRepresentation(rep);return image
    }
    static let pageCurve=CAMediaTimingFunction(controlPoints:0.22,1,0.36,1)
    // Puts the panel at its frame; going to or from a page, the new content slides in from the side it comes from
    // while the old slides away and fades, and the panel eases to its new size.
    private func place(_ frame:NSRect,surface:Surface,previous:NSImage?){
        panel.contentView=surface
        guard let previous,panel.isVisible,!NSWorkspace.shared.accessibilityDisplayShouldReduceMotion else{panel.setFrame(frame,display:true);return}
        let direction=slide,shift:CGFloat=72
        let shot=NSImageView(frame:NSRect(x:0,y:0,width:previous.size.width,height:previous.size.height));shot.image=previous;shot.imageScaling = .scaleNone;shot.imageAlignment = .alignTopLeft
        shot.wantsLayer=true;surface.addSubview(shot)
        surface.content.wantsLayer=true
        func move(_ layer:CALayer?,from:CGFloat,to:CGFloat,fade:(CGFloat,CGFloat),duration:Double,done:(()->Void)?=nil){
            guard let layer else{return}
            CATransaction.begin();CATransaction.setCompletionBlock(done)
            let slide=CABasicAnimation(keyPath:"transform.translation.x");slide.fromValue=from;slide.toValue=to
            let alpha=CABasicAnimation(keyPath:"opacity");alpha.fromValue=fade.0;alpha.toValue=fade.1
            let group=CAAnimationGroup();group.animations=[slide,alpha];group.duration=duration;group.timingFunction=App.pageCurve
            group.fillMode = .forwards;group.isRemovedOnCompletion=false;layer.add(group,forKey:"page")
            CATransaction.commit()
        }
        move(surface.content.layer,from:direction*shift,to:0,fade:(0,1),duration:0.32){[weak surface] in surface?.content.layer?.removeAnimation(forKey:"page")}
        move(shot.layer,from:0,to:-direction*shift*0.6,fade:(1,0),duration:0.22){shot.removeFromSuperview()}
        NSAnimationContext.runAnimationGroup{context in context.duration=0.3;context.timingFunction=App.pageCurve;panel.animator().setFrame(frame,display:true)}
    }
    // A page: a back button with the row's icon, name and figure, then its rows; opening one in place grows the panel.
    private func layoutPage(_ page:Entry,previous:NSImage?){
        closeFlyouts(from:0);rows=[]
        // A refresh redraws the page where the reader left it; arriving on it starts at the top.
        let kept=slide==0 ? (pageScroll?.contentView.bounds.origin.y ?? 0) : 0
        let screen=NSScreen.screens.first(where:{$0.frame.contains(anchor)}) ?? NSScreen.main!,width:CGFloat=360,inner=width-16,header:CGFloat=50
        let detail=DetailView(page.children,width:inner,open:pageOpen,busy:interaction.active)
        let height=min(header+detail.frame.height+20,screen.visibleFrame.height-16)
        let surface=Surface(frame:NSRect(x:0,y:0,width:width,height:height),glass:snapshot?.appearance != "solid")
        let content=surface.content
        let backButton=HintButton(frame:NSRect(x:12,y:13,width:27,height:25))
        backButton.bare=true;backButton.controlSize = .small;backButton.title="";backButton.imagePosition = .imageOnly;backButton.contentTintColor = .labelColor
        backButton.image=NSImage(systemSymbolName:"chevron.left",accessibilityDescription:"返回")?.withSymbolConfiguration(.init(pointSize:11,weight:.semibold))
        backButton.hint="返回";backButton.hovered={[weak self] in self?.hoverHint($0,$1)};backButton.setAccessibilityLabel("返回")
        backButton.target=self;backButton.action=#selector(backPressed);content.addSubview(backButton)
        var titleX:CGFloat=48
        let glyph:NSImage?={
            if let raw=page.image,let data=Data(base64Encoded:raw),let image=NSImage(data:data){image.isTemplate=page.template ?? false;return image}
            return page.symbol.flatMap{NSImage(systemSymbolName:$0,accessibilityDescription:nil)}
        }()
        if let glyph {
            let icon=NSImageView(frame:NSRect(x:titleX,y:16,width:18,height:18));icon.image=glyph;icon.imageScaling = .scaleProportionallyUpOrDown
            if glyph.isTemplate{icon.contentTintColor = .labelColor};content.addSubview(icon);titleX+=25
        }
        // The figure beside the name keeps its whole text, in a shorter form if the name needs the room.
        let titleFont=NSFont.systemFont(ofSize:14,weight:.semibold),valueFont=NSFont.monospacedDigitSystemFont(ofSize:11.5,weight:.medium)
        let room=width-16-titleX-MenuLayout.width(page.label,font:titleFont)-14
        let shown=page.value.map{value in ([value]+(page.fallbacks ?? [])).first{MenuLayout.width($0,font:valueFont)+6<=room} ?? (page.fallbacks?.last ?? value)}
        var valueWidth:CGFloat=0
        if let shown {
            let value=NSTextField(labelWithString:shown);value.font=valueFont;value.textColor=usageColor(page.usageLevel) ?? .secondaryLabelColor;value.alignment = .right
            value.lineBreakMode = .byTruncatingTail;valueWidth=min(max(60,room),MenuLayout.width(shown,font:valueFont)+6)
            value.frame=NSRect(x:width-16-valueWidth,y:18,width:valueWidth,height:16);content.addSubview(value)
        }
        let title=NSTextField(labelWithString:page.label);title.font=titleFont;title.lineBreakMode = .byTruncatingTail
        title.frame=NSRect(x:titleX,y:15,width:width-16-valueWidth-8-titleX,height:20);content.addSubview(title)
        let rule=NSBox(frame:NSRect(x:16,y:header-1,width:width-32,height:1));rule.boxType = .separator;content.addSubview(rule)
        let scroll=ContentScrollView(frame:NSRect(x:8,y:header+4,width:inner,height:height-header-12));scroll.drawsBackground=false;scroll.scrollerStyle = .overlay
        scroll.documentView=detail;scroll.configureScrolling();content.addSubview(scroll);pageScroll=scroll
        if kept>0 {scroll.contentView.scroll(to:NSPoint(x:0,y:min(kept,max(0,detail.frame.height-scroll.frame.height))));scroll.reflectScrolledClipView(scroll.contentView)}
        scroll.swipedBack={[weak self] in self?.back()}
        detail.invoke={[weak self] entry in
            guard let self else{return}
            self.open(entry)
            // Still here (the action kept the panel open): show it running.
            if self.panel.isVisible && self.pageEntry() != nil {self.updatePresentation()}
        }
        detail.chose={[weak self] entry in
            guard let self,let action=entry.action else{return}
            self.quietUntil=Date().addingTimeInterval(0.3);self.performAction(action)
        }
        detail.toggled={[weak self] open in self?.pageOpen=open}
        detail.resized={[weak self,weak scroll,weak surface] body,animated in
            guard let self,let scroll,let surface else{return}
            let total=min(header+body+20,screen.visibleFrame.height-16),frame=self.panel.frame
            let target=NSRect(x:frame.minX,y:frame.maxY-total,width:frame.width,height:total)
            scroll.frame.size.height=max(scroll.frame.height,total-header-12)
            let finish={[weak scroll] in scroll?.frame.size.height=total-header-12;scroll?.configureScrolling()}
            if animated {
                NSAnimationContext.runAnimationGroup({context in context.duration=0.26;context.timingFunction=App.pageCurve;self.panel.animator().setFrame(target,display:true)},completionHandler:finish)
            } else {self.panel.setFrame(target,display:true);finish()}
            _=surface
        }
        let x=min(max(anchor.x-22,screen.visibleFrame.minX+4),screen.visibleFrame.maxX-width-4)
        place(NSRect(x:x,y:screen.visibleFrame.maxY-height-4,width:width,height:height),surface:surface,previous:previous);placeToast()
    }
    @objc private func backPressed(){back()}
    private func topPanel(at point:NSPoint)->NSWindow? {
        let windows=([panel]+flyouts.map{$0.panel}).filter{$0.isVisible}
        guard let index=MenuLayout.topmostIndex(at:point,frames:windows.map{$0.frame}) else{return nil}
        return windows[index]
    }
    private func pointerHits(_ row:Row)->Bool {topPanel(at:NSEvent.mouseLocation) === row.window && rowRect(row).contains(NSEvent.mouseLocation)}
    private func allowsConfigurationHover(_ row:Row)->Bool {
        guard let window=row.window else{return false}
        // In the module grid the way to a sidebar crosses other modules, so settings open on a click instead.
        if managerOnly && window === panel {return false}
        return MenuLayout.configurationHover(row.entry,x:row.convert(window.convertPoint(fromScreen:NSEvent.mouseLocation),from:nil).x)
    }
    // Rows open their page on a click; the pointer only lights them.
    private func hover(_ entry:Entry,row:Row,depth:Int){}
    private func recordPointer(_ point:NSPoint){
        let now=ProcessInfo.processInfo.systemUptime
        if trail.last?.point != point{trail.append((point,now))}
        trail.removeAll{now-$0.time>0.25}
    }
    private func aimingAtFlyout(_ depth:Int)->Bool {
        guard flyouts.indices.contains(depth),let current=trail.last else{return false}
        let now=ProcessInfo.processInfo.systemUptime
        // A resting pointer is choosing the row under it, not heading anywhere.
        guard now-current.time<0.06,let start=trail.first(where:{now-$0.time<=0.12}) else{return false}
        return MenuLayout.aiming(from:start.point,to:current.point,flyout:flyouts[depth].panel.frame)
    }
    private func openFlyout(_ entry:Entry,row:Row,depth:Int,checkSource:Bool=true){
        guard !entry.children.isEmpty,let parent=row.window else{return}
        if flyouts.indices.contains(depth),flyouts[depth].key==entry.key{return}
        closeFlyouts(from:depth);row.expanded=true;row.yielded=false
        let window=Panel(contentRect:.zero,styleMask:[.borderless,.nonactivatingPanel],backing:.buffered,defer:false);setup(window);window.setAccessibilityLabel(entry.label)
        let screen=parent.screen ?? NSScreen.main!,width=MenuLayout.flyoutWidth(entry),body=entry.children.reduce(CGFloat(0)){$0+MenuLayout.rowHeight($1,width:MenuLayout.flyoutWidth(entry)-16,left:MenuLayout.left($1,entry.children))}
        let height=min(body+40,screen.visibleFrame.height-16),surface=Surface(frame:NSRect(x:0,y:0,width:width,height:height),glass:self.snapshot?.appearance != "solid")
        let heading=NSTextField(labelWithString:entry.label);heading.font = .systemFont(ofSize:11,weight:.medium);heading.textColor = .secondaryLabelColor;let headingLeft=MenuLayout.column(entry.children)+8;heading.frame=NSRect(x:headingLeft,y:10,width:width-headingLeft-20,height:18);surface.content.addSubview(heading)
        let scroll=ContentScrollView(frame:NSRect(x:8,y:32,width:width-16,height:height-40));scroll.drawsBackground=false;scroll.hasVerticalScroller=body>scroll.frame.height;scroll.scrollerStyle = .overlay
        let content=FlippedView(frame:NSRect(x:0,y:0,width:width-16,height:body));_=fillRows(entry.children,in:content,y:0,width:width-16,depth:depth+1);scroll.documentView=content;scroll.configureScrolling();surface.content.addSubview(scroll);window.contentView=surface
        let ancestor=depth>0 ? (depth==1 ? panel.frame : flyouts[depth-2].panel.frame) : nil
        let x=MenuLayout.flyoutX(parent:parent.frame,ancestor:ancestor,width:width,screen:screen.visibleFrame)
        let rect=rowRect(row),y=max(screen.visibleFrame.minY+4,min(rect.maxY+6-height,screen.visibleFrame.maxY-height-4))
        window.setFrame(NSRect(x:x,y:y,width:width,height:height),display:true);parent.addChildWindow(window,ordered:.above);window.orderFront(nil);flyouts.append(Flyout(panel:window,key:entry.key,row:row))
        if checkSource,let action=entry.onOpenAction,action.count==2 {
            let key=action[1],now=Date()
            if now.timeIntervalSince(sourceChecks[key] ?? .distantPast)>=5 {
                sourceChecks[key]=now
                DispatchQueue.main.async{[weak self] in self?.performAction(action)}
            }
        }
    }
    private func closeFlyouts(from depth:Int){
        guard depth<flyouts.count else{return}
        for flyout in flyouts[depth...].reversed(){flyout.row?.expanded=false;flyout.row?.yielded=false;flyout.panel.parent?.removeChildWindow(flyout.panel);flyout.panel.orderOut(nil)}
        flyouts.removeSubrange(depth...);if rows.count>depth+1{rows.removeSubrange((depth+1)...)}
    }
    private func checkPointer(){
        guard panel.isVisible,!anchoredMenuOpen else{return}
        let p=NSEvent.mouseLocation,top=topPanel(at:NSEvent.mouseLocation);recordPointer(p)
        let hit=rows.indices.reversed().lazy.compactMap{depth in self.rows[depth].values.first(where:{$0.window === top && self.rowRect($0).contains(p)}).map{(depth:depth,row:$0)}}.first
        // The highlight follows the pointer at once, unless it is crossing rows on its way into the open flyout.
        for (depth,flyout) in flyouts.enumerated(){flyout.row?.yielded=hit.map{$0.row !== flyout.row && $0.row.window === flyout.row?.window && !aimingAtFlyout(depth)} ?? false}
        for group in rows{for row in group.values{row.needsDisplay=true}}
        if case let (depth,row)? = hit {
            closeTask?.cancel()
            if !allowsConfigurationHover(row){hoverIntent.cancel();return}
            if row.entry.children.isEmpty {
                if flyouts.count>depth{hover(row.entry,row:row,depth:depth)}
            }else if !flyouts.indices.contains(depth) || flyouts[depth].key != row.entry.key {
                hover(row.entry,row:row,depth:depth)
            }
            return
        }
        guard !flyouts.isEmpty else{return}
        // Inside a panel but off its rows (Summary, gaps, a flyout heading): flyouts beyond that panel
        // close shortly, unless the pointer is heading into them. Outside every panel, all of them close.
        guard let top else{scheduleClose(from:0,after:0.15);return}
        let level=top === panel ? 0 : (flyouts.firstIndex{$0.panel === top}.map{$0+1} ?? 0)
        // A module's settings, opened by a click, stay open while the pointer crosses the grid to them.
        if flyouts.count<=level || aimingAtFlyout(level) || (managerOnly && level==0){closeTask?.cancel();return}
        scheduleClose(from:level,after:0.12)
    }
    private func scheduleClose(from level:Int,after delay:TimeInterval){
        if let closeTask,!closeTask.isCancelled,closeLevel==level{return}
        closeTask?.cancel();closeLevel=level
        let task=DispatchWorkItem{[weak self] in self?.closeFlyouts(from:level);self?.closeTask=nil}
        closeTask=task;DispatchQueue.main.asyncAfter(deadline:.now()+delay,execute:task)
    }
    private func checkAction(){
        guard let id=pendingAction else{return}
        let receipt=URL(fileURLWithPath:snapshotPath).deletingLastPathComponent().appendingPathComponent("panel-action-receipt.json")
        guard let data=try? Data(contentsOf:receipt),let value=try? JSONDecoder().decode(ActionReceipt.self,from:data),value.id==id else{return}
        finishAction(id,preferences:value.preferences,failed:value.status != "done")
    }
    private func finishAction(_ id:String,preferences:PresentationPreferences?=nil,failed:Bool=false){
        guard pendingAction==id else{return}
        let report=failed && !PanelInteraction.automatic(interaction.active ?? [])
        actionTimeout?.cancel();pendingAction=nil;interaction.finish(preferences);updateFallback()
        if panel.isVisible{reload(force:true)}
        if report {showActionFailure(macHost==nil ? "SwiftBar 未完成这次操作，请重试。" : "未完成这次操作，请重试。")}
        if let next=interaction.next(){dispatchAction(next)}
    }
    private func showActionFailure(_ detail:String){
        let alert=NSAlert();alert.messageText="操作未完成";alert.informativeText=detail;alert.addButton(withTitle:"好");alert.runModal()
    }
    private func activate(_ entry:Entry,row:Row,depth:Int){
        if entry.isModule,!row.configurationClick,let action=entry.action{performAction(action);return}
        if !entry.children.isEmpty{push(entry);return}
        open(entry)
    }
    // What a row without a page does: open the app or the link, or run its action.
    private func open(_ entry:Entry){
        // Desktop app update rows link to the installed .app, which opens the app to update itself.
        if let href=entry.href,let url=URL(string:href),url.isFileURL,url.pathExtension=="app"{
            hide(returningFocus:false)
            NSWorkspace.shared.openApplication(at:url,configuration:NSWorkspace.OpenConfiguration()){[weak self] _,error in
                if error != nil{DispatchQueue.main.async{self?.showActionFailure("无法打开应用。请确认应用已安装后重试。")}}
            };return
        }
        if let href=entry.href,let url=URL(string:href),["http","https","swiftbar"].contains(url.scheme ?? ""){
            // Web pages come to the front; SwiftBar commands run without taking focus from the user's app.
            let config=NSWorkspace.OpenConfiguration();config.activates=url.scheme != "swiftbar"
            hide(returningFocus:!config.activates)
            NSWorkspace.shared.open(url,configuration:config){[weak self] _,error in
                if error != nil{DispatchQueue.main.async{self?.showActionFailure("系统无法打开此链接，请重试。")}}
            };return
        }
        if let action=entry.action{performAction(action)}
    }
    @objc private func rangeTrackingBegan(_ notification:Notification){if let menu=notification.object as? NSMenu,menu === rangePicker?.menu{menuWillOpen(menu)}}
    @objc private func rangeTrackingEnded(_ notification:Notification){if let menu=notification.object as? NSMenu,menu === rangePicker?.menu{menuDidClose(menu)}}
    func menuWillOpen(_ menu:NSMenu){anchoredMenuOpen=true;hideHint();hoverIntent.cancel();closeTask?.cancel();closeFlyouts(from:0)}
    func menuDidClose(_ menu:NSMenu){anchoredMenuOpen=false;DispatchQueue.main.async{[weak self] in self?.reload(force:true)}}
    // A short label under a header button, 350 ms after the pointer settles on it.
    private func hideHint(){hintTask?.cancel();hintTask=nil;hintView?.removeFromSuperview();hintView=nil}
    private func hoverHint(_ button:HintButton,_ inside:Bool){
        hideHint()
        guard inside,!button.hint.isEmpty,!anchoredMenuOpen else{return}
        let task=DispatchWorkItem{[weak self,weak button] in
            guard let self,!self.anchoredMenuOpen,let button,let surface=button.window?.contentView as? Surface else{return}
            // The bubble is sized from the drawn text and the label spans it centred, so both sides get the same space.
            let label=NSTextField(labelWithString:button.hint);label.font = .systemFont(ofSize:11);label.textColor = .labelColor;label.alignment = .center
            let text=(button.hint as NSString).size(withAttributes:[.font:label.font!]),height=ceil(label.intrinsicContentSize.height)
            let anchor=surface.convert(button.bounds,from:button),width=ceil(text.width)+24
            let bubble=FlippedView(frame:NSRect(x:min(max(8,anchor.midX-width/2),surface.bounds.width-width-8),y:anchor.maxY+4,width:width,height:height+8))
            bubble.wantsLayer=true;bubble.layer?.cornerRadius=6;bubble.layer?.borderWidth=0.5
            surface.effectiveAppearance.performAsCurrentDrawingAppearance{bubble.layer?.backgroundColor=background(surface).cgColor;bubble.layer?.borderColor=NSColor.separatorColor.cgColor}
            label.frame=NSRect(x:0,y:4,width:width,height:height);bubble.addSubview(label)
            surface.addSubview(bubble);self.hintView=bubble
        }
        hintTask=task;DispatchQueue.main.asyncAfter(deadline:.now()+0.35,execute:task)
    }
    @objc private func refreshData(){performAction(["refresh"])}
    @objc private func detectModules(){performAction(["detect-modules"])}
    // The picture covers Summary and the Harness cards only; subscriptions, balances and keys stay private.
    private func sharePicture(_ style:ShareCard.Style)->Data? {
        guard let dashboard=snapshot?.dashboard else{return nil}
        let card=ShareCard.make(style,dashboard,project:snapshot?.project ?? Project(name:"Starmeter",url:"https://github.com/timecatbit/starmeter"))
        card.appearance=panel.effectiveAppearance
        return card.png()
    }
    // Copy lives in our own menu: the system share sheet runs in another process, and its clicks
    // used to look like clicks outside the panel, closing it before the copy ran.
    // Each style has its own copy and share, so one click makes the picture people picked.
    @objc private func shareDashboard(_ sender:NSButton){
        let menu=NSMenu();menu.autoenablesItems=false;menu.delegate=self
        for style in ShareCard.Style.allCases {
            if style != ShareCard.Style.allCases.first {menu.addItem(.separator())}
            menu.addItem(Self.menuHeader(style.title))
            for (title,symbol,action) in [("复制图片","doc.on.doc",#selector(copySharePicture(_:))),("分享…","square.and.arrow.up",#selector(openSharePicker(_:)))]{
                let item=NSMenuItem(title:title,action:action,keyEquivalent:"");item.target=self;item.representedObject=sender;item.tag=style.rawValue
                item.image=NSImage(systemSymbolName:symbol,accessibilityDescription:nil);menu.addItem(item)
            }
        }
        menu.popUp(positioning:nil,at:NSPoint(x:0,y:sender.bounds.height+4),in:sender)
    }
    // A group title in a menu; macOS 13 has no section headers, so it gets a quiet disabled row.
    static func menuHeader(_ title:String)->NSMenuItem {
        if #available(macOS 14.0,*) {return NSMenuItem.sectionHeader(title:title)}
        let item=NSMenuItem(title:title,action:nil,keyEquivalent:"");item.isEnabled=false
        item.attributedTitle=NSAttributedString(string:title,attributes:[.font:NSFont.systemFont(ofSize:NSFont.smallSystemFontSize,weight:.semibold),.foregroundColor:NSColor.secondaryLabelColor])
        return item
    }
    @objc private func copySharePicture(_ item:NSMenuItem){
        guard let data=sharePicture(ShareCard.Style(rawValue:item.tag) ?? .card) else{NSSound.beep();return}
        NSPasteboard.general.clearContents()
        let copied=NSPasteboard.general.writeObjects([ShareCard.pasteboardItem(data)])
        showToast(copied ? "已复制" : "复制失败",ok:copied)
    }
    @objc private func openSharePicker(_ item:NSMenuItem){
        guard let sender=item.representedObject as? NSButton,sender.window != nil,let data=sharePicture(ShareCard.Style(rawValue:item.tag) ?? .card),let dashboard=snapshot?.dashboard else{NSSound.beep();return}
        let url=FileManager.default.temporaryDirectory.appendingPathComponent("Starmeter "+dashboard.date+".png")
        guard (try? data.write(to:url,options:.atomic)) != nil else{NSSound.beep();return}
        let picker=NSSharingServicePicker(items:[url]);picker.delegate=self
        anchoredMenuOpen=true;hoverIntent.cancel();closeTask?.cancel();closeFlyouts(from:0)
        picker.show(relativeTo:sender.bounds,of:sender,preferredEdge:.minY)
    }
    func sharingServicePicker(_ picker:NSSharingServicePicker,didChoose service:NSSharingService?){
        anchoredMenuOpen=false;DispatchQueue.main.async{[weak self] in self?.reload(force:true)}
    }
    // A short confirmation on the panel surface; a rebuild re-places it until it expires.
    // Copy feedback stays in the header row so it never covers the numbers: the share button briefly
    // turns into a green check (orange on failure) and a short word sits in the gap left of the buttons.
    private func showToast(_ text:String,ok:Bool){
        toast=(text,Date().addingTimeInterval(1.8),ok);placeToast()
        DispatchQueue.main.asyncAfter(deadline:.now()+1.85){[weak self] in
            guard let self,let toast=self.toast,toast.until<=Date() else{return}
            self.toast=nil;self.placeToast()
        }
    }
    private func placeToast(){
        toastView?.removeFromSuperview();toastView=nil
        let current=toast.flatMap{$0.until>Date() ? $0 : nil}
        if let share=shareButton {
            // Bordered buttons ignore contentTintColor for their image, so the status icon carries its own colour.
            let base=NSImage.SymbolConfiguration(pointSize:11,weight:.semibold)
            if let current {
                let colour=current.ok ? statusGreen(share) : NSColor.systemOrange
                share.image=NSImage(systemSymbolName:current.ok ? "checkmark" : "exclamationmark",accessibilityDescription:current.text)?
                    .withSymbolConfiguration(base.applying(.init(paletteColors:[colour])))
            } else {share.image=NSImage(systemSymbolName:"square.and.arrow.up",accessibilityDescription:"分享用量图")?.withSymbolConfiguration(base)}
        }
        guard let current,let surface=panel.contentView as? Surface,let detect=settingsButton ?? pinButton ?? detectButton,let row=detect.superview else{return}
        let label=NSTextField(labelWithString:current.text);label.font = .systemFont(ofSize:11,weight:.medium)
        label.textColor=current.ok ? statusGreen(surface) : .systemOrange
        let size=label.intrinsicContentSize,anchor=surface.convert(detect.bounds,from:detect)
        // Only in the gap after the heading; with a long heading the button alone carries the feedback.
        let heading=(snapshot?.dashboard.map(DashboardView.heading) ?? "Summary") as NSString
        let headingRight=surface.convert(NSPoint(x:12,y:0),from:row).x+ceil(heading.size(withAttributes:[.font:NSFont.systemFont(ofSize:14,weight:.semibold)]).width)+6
        // A little slack in the width keeps the last glyph from being clipped; the gap keeps it clear of the buttons.
        let width=ceil(size.width)+4,x=anchor.minX-12-width
        NSAccessibility.post(element:shareButton ?? surface,notification:.announcementRequested,userInfo:[.announcement:current.text])
        guard x>=headingRight else{return}
        label.frame=NSRect(x:x,y:anchor.midY-ceil(size.height)/2,width:width,height:ceil(size.height))
        surface.addSubview(label);toastView=label
    }
    @objc private func chooseRange(_ item:NSMenuItem){if let action=item.representedObject as? [String]{performAction(action)}}
    private func performAction(_ action:[String]){
        switch interaction.submit(action) {
        case .start:dispatchAction(action)
        case .queued:updatePresentation()
        case .ignored:break
        }
    }
    private func dispatchAction(_ action:[String]){
        let passive=["refresh-source","refresh-view","refresh","refresh-live","refresh-monitor","check-app-updates","set-range","set-group","retry-claude-usage"].contains(action[0])
        let keepOpen=PanelInteraction.keepsPanelOpen(action)
        if !keepOpen{hide()};if keepOpen && !passive{updatePresentation()}
        let id=UUID().uuidString,request=URL(fileURLWithPath:snapshotPath).deletingLastPathComponent().appendingPathComponent("panel-action.json")
        pendingAction=id
        do{
            let data=try JSONSerialization.data(withJSONObject:["id":id,"action":action,"createdAt":Date().timeIntervalSince1970])
            try data.write(to:request,options:.atomic)
            // The containing directory is private. A very fast SwiftBar can claim the request before chmod.
            try? FileManager.default.setAttributes([.posixPermissions:0o600],ofItemAtPath:request.path)
            updateFallback()
            if let macHost{macHost.action()}else{wakeSwiftBar()}
            if macHost==nil{DispatchQueue.main.asyncAfter(deadline:.now()+0.8){[weak self] in
                guard let self,self.pendingAction==id,FileManager.default.fileExists(atPath:request.path) else{return}
                // Retry only the notification. SwiftBar atomically claims the same file at most once.
                self.wakeSwiftBar()
            }}
            let task=DispatchWorkItem{[weak self] in
                guard let self,self.pendingAction==id else{return}
                self.checkAction()
                guard self.pendingAction==id else{return}
                let claimed = !FileManager.default.fileExists(atPath:request.path)
                guard PanelInteraction.mayExpire(action,claimed:claimed) else{return}
                if !claimed{try? FileManager.default.removeItem(at:request)}
                self.finishAction(id,failed:true)
            }
            actionTimeout=task
            let timeout:TimeInterval=PanelInteraction.automatic(action) ? 3 : PanelInteraction.isPreference(action) ? 20 : 8
            DispatchQueue.main.asyncAfter(deadline:.now()+timeout,execute:task)
        }catch{finishAction(id,failed:true)}
    }
    private func wakeSwiftBar(){
        var url=URLComponents();url.scheme="swiftbar";url.host="refreshplugin";url.queryItems=[URLQueryItem(name:"name",value:PanelInteraction.swiftBarName(pluginPath))]
        // Refresh without activating SwiftBar or taking focus from the panel.
        let quiet=NSWorkspace.OpenConfiguration();quiet.activates=false
        if let target=url.url{NSWorkspace.shared.open(target,configuration:quiet)}
    }
}
// Application entry point. The interaction tests compile the declarations above without launching the app.
UserDefaults.standard.setVolatileDomain(["AppleAccentColor":5,"AppleAquaColorVariant":1],forName:UserDefaults.argumentDomain)
let app=NSApplication.shared;app.setActivationPolicy(.accessory);let delegate=App();app.delegate=delegate;app.run()
