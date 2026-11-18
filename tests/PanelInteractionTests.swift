// Runs against the actual panel state machine, without opening a window or querying user data.
var assertions=0
func expect(_ condition:@autoclosure ()->Bool,_ message:String){assertions+=1;if !condition(){fatalError(message)}}
let fixture="""
{"dashboard":{"date":"","dates":[],"providers":[],"note":"","rangeKey":"today","rangeLabel":"今天","chartUnit":"","chartSpan":"","chartStatus":"","allowCollapse":true,"expandedProviders":[]},"entries":[],"moduleOptions":[{"label":"允许折叠","checked":true,"action":["set-collapse","false"],"children":[]}],"preferences":{"revision":1,"visible":["summary","opencodex","claude"],"appearance":"glass","allowCollapse":true,"expandedProviders":[]}}
"""
let initial=try JSONDecoder().decode(Snapshot.self,from:Data(fixture.utf8))
func saved(_ revision:Int64,_ expanded:[String])->PresentationPreferences{PresentationPreferences(revision:revision,visible:["summary","opencodex","claude"],appearance:"glass",allowCollapse:true,expandedProviders:expanded)}
func expanded(_ state:PanelInteraction)->Set<String>{Set(state.displayed()?.dashboard?.expandedProviders ?? [])}
var state=PanelInteraction(source:initial)
expect(state.submit(["set-expanded","opencodex","true"]) == .start,"First click must start")
expect(state.submit(["set-expanded","claude","true"]) == .queued,"Second module must queue")
expect(expanded(state)==Set(["opencodex","claude"]),"Both modules must expand immediately")
state.finish(saved(2,["opencodex"]))
state.source=initial // The old data render arrives after the save receipt.
expect(expanded(state)==Set(["opencodex","claude"]),"A stale render must not revert saved or queued state")
expect(state.next()==["set-expanded","claude","true"],"Queue must retain the next module")
expect(expanded(state)==Set(["opencodex","claude"]),"Starting queued action must not close the first module")
state.finish(saved(3,["opencodex","claude"]))
expect(expanded(state)==Set(["opencodex","claude"]),"Saved multiple expansions survive stale data")
expect(state.submit(["set-expanded","opencodex","false"]) == .start,"Close one module")
expect(expanded(state)==Set(["claude"]),"Closing one module must not close the other")
expect(state.submit(["set-expanded","opencodex","true"]) == .queued,"Rapid reopen queues")
expect(state.submit(["set-expanded","opencodex","false"]) == .queued,"Rapid reclose queues")
expect(expanded(state)==Set(["claude"]),"Newest click wins optimistically")
state.finish(saved(4,["claude"]))
_ = state.next() // Older reopen is now active, newer reclose remains queued.
expect(expanded(state)==Set(["claude"]),"Dispatching an older click must not override a newer queued click")
state.finish(saved(5,["opencodex","claude"]))
_ = state.next()
expect(expanded(state)==Set(["claude"]),"Last queued close still wins after the previous save")
state.finish(saved(6,["claude"]))
expect(state.next()==nil,"Queue drains")
var newer=initial;newer.preferences=saved(7,["opencodex"]);state.source=newer
expect(expanded(state)==Set(["opencodex"]),"Newer external settings override an older acknowledged state")
var busy=PanelInteraction(source:initial)
expect(busy.submit(["refresh-source","komari"]) == .start,"Resource check starts")
expect(busy.submit(["set-range","7d"]) == .queued,"Range click must not disappear during resource check")
expect(busy.submit(["refresh-view"]) == .ignored,"An automatic check cannot queue ahead of user actions")
expect(PanelInteraction.swiftBarName("/Users/a/软件:配置/swiftbar/starmeter.30s.sh")=="starmeter","Panel wakes SwiftBar by the plugin name every version matches")
expect(PanelInteraction.mayExpire(["refresh-source","komari"],claimed:true),"A lost automatic receipt cannot stall the panel")
expect(PanelInteraction.mayExpire(["reauthorize-claude-oauth"],claimed:false),"An unclaimed authorization click can time out")
expect(!PanelInteraction.mayExpire(["reauthorize-claude-oauth"],claimed:true),"A visible authorization prompt can wait for a decision")
expect(busy.submit(["open-settings"]) == .queued,"Menu action must not disappear during resource check")
expect(busy.submit(["open-settings"]) == .ignored,"Repeated non-preference action is not duplicated")
expect(busy.submit(["set-expanded","claude","true"]) == .queued,"Expansion during check queues")
expect(expanded(busy)==Set(["claude"]),"Expansion still responds immediately")
busy.finish(nil);expect(busy.next()==["set-range","7d"],"Range dispatches in order")
busy.finish(nil);expect(busy.next()==["open-settings"],"Menu action dispatches in order")
busy.finish(nil);expect(busy.next()==["set-expanded","claude","true"],"Expansion is eventually saved")
busy.finish(saved(2,["claude"]));expect(busy.next()==nil,"Background queue drains")
_ = busy.submit(["set-collapse","false"])
expect(busy.displayed()?.dashboard?.allowCollapse==false,"Disable collapse immediately")
expect(expanded(busy)==Set(["claude"]),"Disable collapse preserves independent state")
let scroll=ContentScrollView(frame:NSRect(x:0,y:0,width:200,height:150))
let document=FlippedView(frame:NSRect(x:0,y:0,width:200,height:100));scroll.documentView=document;scroll.configureScrolling()
expect(!scroll.needsVerticalScrolling,"Content that fits must not scroll")
expect(scroll.verticalScrollElasticity == .none,"Content that fits must not bounce")
let wheel=CGEvent(scrollWheelEvent2Source:nil,units:.pixel,wheelCount:1,wheel1:80,wheel2:0,wheel3:0)!
scroll.scrollWheel(with:NSEvent(cgEvent:wheel)!)
expect(scroll.contentView.bounds.origin == .zero,"Two-finger scroll must leave fitting content still")
document.setFrameSize(NSSize(width:200,height:500));scroll.configureScrolling()
expect(scroll.needsVerticalScrolling,"Overflow must remain scrollable")
scroll.contentView.scroll(to:NSPoint(x:0,y:120));document.setFrameSize(NSSize(width:200,height:100));scroll.configureScrolling()
expect(scroll.contentView.bounds.origin == .zero,"Collapsing content must clear obsolete scroll offset")
expect(!scroll.hasVerticalScroller,"No scrollbar when everything fits")
let watchDirectory=FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
try FileManager.default.createDirectory(at:watchDirectory,withIntermediateDirectories:true)
let observer=SnapshotObserver();var notifications=0
observer.start(directory:watchDirectory.path){notifications+=1}
expect(observer.active,"Directory observer starts without a polling timer")
try Data("first".utf8).write(to:watchDirectory.appendingPathComponent("panel-view.json"),options:.atomic)
let deadline=Date().addingTimeInterval(2)
while notifications==0 && Date()<deadline{RunLoop.main.run(until:Date().addingTimeInterval(0.02))}
expect(notifications>0,"Atomic snapshot replacement wakes the observer")
let count=notifications
RunLoop.main.run(until:Date().addingTimeInterval(0.2))
expect(notifications==count,"Idle directory produces no recurring wakeups")
try FileManager.default.removeItem(at:watchDirectory)
let removedDeadline=Date().addingTimeInterval(2)
while observer.active && Date()<removedDeadline{RunLoop.main.run(until:Date().addingTimeInterval(0.02))}
expect(!observer.active,"Removing the cache directory releases a stale watch")
try FileManager.default.createDirectory(at:watchDirectory,withIntermediateDirectories:true)
observer.start(directory:watchDirectory.path){notifications+=1}
expect(observer.active,"A recreated cache directory can be watched again")
observer.stop();expect(!observer.active,"Observer releases its file descriptor on stop")
try FileManager.default.removeItem(at:watchDirectory)
let monitor=NSRect(x:0,y:0,width:1200,height:900)
let rootFrame=NSRect(x:900,y:200,width:280,height:650)
let secondFrame=NSRect(x:671,y:100,width:224,height:616)
let thirdX=MenuLayout.flyoutX(parent:secondFrame,ancestor:rootFrame,width:260,screen:monitor)
expect(thirdX+260<secondFrame.minX,"Third level continues left instead of covering the root")
let rightRoot=NSRect(x:10,y:200,width:280,height:650),rightSecond=NSRect(x:295,y:100,width:224,height:600)
expect(MenuLayout.flyoutX(parent:rightSecond,ancestor:rightRoot,width:260,screen:monitor)>rightSecond.maxX,"Right-opening menus keep opening right")
let tight=NSRect(x:0,y:0,width:500,height:900)
let tightX=MenuLayout.flyoutX(parent:NSRect(x:20,y:0,width:224,height:600),ancestor:rootFrame,width:260,screen:tight)
expect(tightX>=4 && tightX+260<=496,"Constrained flyouts stay within the screen")
let overlap=NSRect(x:950,y:400,width:224,height:200),point=NSPoint(x:1000,y:450)
expect(MenuLayout.topmostIndex(at:point,frames:[rootFrame,overlap])==1,"Covered rows cannot own hover or mouse input")
expect(MenuLayout.topmostIndex(at:NSPoint(x:1000,y:650),frames:[rootFrame,overlap])==0,"Uncovered ancestor rows remain reachable")
expect(MenuLayout.topmostIndex(at:NSPoint(x:100,y:100),frames:[rootFrame,overlap])==nil,"Outside points do not hit a menu")
var grouped=initial.dashboard!
let sampleProvider=ProviderUsage(id:"claude",name:"Claude",subtitle:"",status:"",available:false,cost:nil,tokens:0,requests:0,output:0,cache:nil,coverage:nil,sessions:0,unpriced:0,summaryIncluded:true,models:[],history:[])
expect(DashboardView.harnessHeaderHeight(grouped)==0,"Empty Harness group has no header")
grouped.providers=[sampleProvider]
expect(DashboardView.harnessHeaderHeight(grouped)==6,"Harness rows get a small gap instead of a heading")
let groupedView=DashboardView(grouped,width:344)
let disclosure=groupedView.subviews.compactMap{$0 as? DisclosureButton}.first!
expect(disclosure.frame.minY==DashboardView.summaryHeight(grouped)+DashboardView.harnessHeaderHeight(grouped)+2,"Disclosure hitbox follows the gap above the Harness rows")
grouped.showSummary=false
expect(DashboardView.harnessHeaderHeight(grouped)==0,"Hidden Summary uses the title without a duplicate Harness header")
expect(MenuLayout.configured("已配置")==true,"Configured gets a filled indicator")
expect(MenuLayout.configured("未配置")==false,"Unconfigured gets an outline indicator")
expect(MenuLayout.configured("已同步")==nil,"Connection results must not be confused with configuration")
let configuredEntry=try JSONDecoder().decode(Entry.self,from:Data("{\"label\":\"ChatGPT\",\"value\":\"已配置\",\"symbol\":\"folder\",\"children\":[{\"label\":\"目录\",\"children\":[]}]}".utf8))
expect(MenuLayout.valueWidth(configuredEntry)==MenuLayout.width("已配置",font:MenuLayout.valueFont)+12,"Indicator reserves its own column")
expect(MenuLayout.labelWidth(configuredEntry,width:280,left:36)+36+MenuLayout.trailing(configuredEntry)+MenuLayout.valueWidth(configuredEntry)+10==280,"Status, label and arrow cannot overlap")
// Two keys with 19-character names need far more than a homepage row; the value gives way, never the title.
let longNames=try JSONDecoder().decode(Entry.self,from:Data("{\"label\":\"Kimi API\",\"value\":\"余额 一二三四五六七八九十一二三四五六七八九 ¥100.00 · 九八七六五四三二一十九八七六五四三二一 ¥99.99\",\"fallbacks\":[\"余额 ¥100.00 · ¥99.99\",\"2 把 Key\"],\"children\":[{\"label\":\"K0\",\"children\":[]}]}".utf8))
let shown=MenuLayout.fitted(longNames,width:344,left:36)
expect(shown.value=="余额 ¥100.00 · ¥99.99","Key names that don't fit give way to the bare amounts")
expect(MenuLayout.labelWidth(shown,width:344,left:36)>=MenuLayout.width("Kimi API",font:MenuLayout.labelFont(shown)),"The title keeps its full width beside the value")
expect(MenuLayout.rowHeight(longNames,width:344,left:36)==MenuLayout.rowHeight(shown,width:344,left:36),"Rows are measured with the value they show")
var many=longNames;many.fallbacks=["余额 ¥1000000.00 · ¥2000000.00 · ¥3000000.00 · ¥4000000.00","4 把 Key"]
expect(MenuLayout.fitted(many,width:344,left:36).value=="4 把 Key","Amounts that still don't fit give way to the key count")
var roomy=longNames;roomy.value="余额 默认 ¥228.37 · lab ¥-0.69"
expect(MenuLayout.fitted(roomy,width:344,left:36).value==roomy.value,"Names that fit stay listed")
let mergedEntry=try JSONDecoder().decode(Entry.self,from:Data("{\"label\":\"ChatGPT 订阅\",\"checked\":false,\"value\":\"已配置\",\"action\":[\"set-module\",\"chatgpt\",\"true\"],\"children\":[{\"label\":\"选择配置目录\",\"action\":[\"choose-client-home\",\"chatgpt\"],\"children\":[]}]}".utf8))
expect(!MenuLayout.configurationClick(mergedEntry,x:19,width:264),"Checkbox click toggles rather than opening settings")
expect(!MenuLayout.configurationClick(mergedEntry,x:110,width:264),"Label click retains multi-select behavior")
expect(MenuLayout.configurationClick(mergedEntry,x:247,width:264),"Arrow click opens settings rather than toggling")
expect(!MenuLayout.configurationHover(mergedEntry,x:19),"Hovering a checkbox must not open a flyout")
expect(MenuLayout.configurationHover(mergedEntry,x:110),"Hovering the label opens configuration")
var mergedSnapshot=initial;mergedSnapshot.moduleOptions=[mergedEntry]
var mergedState=PanelInteraction(source:mergedSnapshot)
_ = mergedState.submit(["set-module","chatgpt","true"])
let selectedEntry=mergedState.displayed()!.moduleOptions![0]
expect(selectedEntry.checked==true,"Merged row responds to a toggle immediately")
expect(selectedEntry.children.first?.action==["choose-client-home","chatgpt"],"Optimistic selection preserves connection actions")
expect(selectedEntry.value=="已配置","Selecting does not change configuration state")
expect(selectedEntry.key==mergedEntry.key,"Open configuration retains its anchor after the checkbox changes")
expect(MenuLayout.rowHeight(mergedEntry,width:264,left:36)==27,"Status and checkmark fit on a single compact row")
expect(usageColor("normal")==NSColor.labelColor,"Normal usage adapts to light and dark appearance")
expect(usageColor("warning")==NSColor.systemOrange,"High usage uses system orange")
expect(usageColor("critical")==NSColor.systemRed,"Critical usage uses system red")
expect(usageColor(nil)==nil,"Stale or missing usage keeps its neutral state")
// Hover follows the pointer: the delay counts from entering a row, not from the last movement.
// Which closure fires tells whether a delay restarted, so each step waits for its firing instead of a
// fixed time that a busy CI machine can overrun.
let intent=HoverIntent();intent.delay=0.2;var fired:[String]=[]
func awaitFired(_ count:Int){let deadline=Date().addingTimeInterval(2);while fired.count<count && Date()<deadline{RunLoop.main.run(until:Date().addingTimeInterval(0.02))}}
intent.schedule("0:a"){fired.append("a")}
intent.schedule("0:a"){fired.append("a-restarted")}
awaitFired(1)
expect(fired==["a"],"Moving inside a row must not restart its hover delay")
intent.schedule("0:b"){fired.append("b")};intent.schedule("0:c"){fired.append("c")}
awaitFired(2)
expect(fired==["a","c"],"Only the row the pointer reached last may open")
intent.schedule("0:a"){fired.append("a-again")}
awaitFired(3)
expect(fired.last=="a-again","Returning to a row after its flyout opened schedules it again")
intent.schedule("0:b"){fired.append("cancelled")};intent.cancel()
RunLoop.main.run(until:Date().addingTimeInterval(0.3))
expect(fired.last=="a-again" && intent.target==nil,"Cancelled hover never fires")
expect(MenuLayout.highlighted(pointerInside:false,expanded:true,yielded:true)==false,"An open row yields its highlight to the hovered sibling")
expect(MenuLayout.highlighted(pointerInside:false,expanded:true,yielded:false),"An open row stays highlighted while its flyout is in use")
expect(MenuLayout.highlighted(pointerInside:true,expanded:false,yielded:false),"The row under the pointer is highlighted at once")
let openFlyout=NSRect(x:400,y:300,width:240,height:200)
expect(MenuLayout.aiming(from:NSPoint(x:330,y:480),to:NSPoint(x:350,y:470),flyout:openFlyout),"Diagonal movement into the flyout keeps it open")
expect(!MenuLayout.aiming(from:NSPoint(x:350,y:480),to:NSPoint(x:350,y:450),flyout:openFlyout),"Moving straight down the list switches rows")
expect(!MenuLayout.aiming(from:NSPoint(x:350,y:480),to:NSPoint(x:330,y:470),flyout:openFlyout),"Moving away from the flyout switches rows")
expect(!MenuLayout.aiming(from:NSPoint(x:330,y:480),to:NSPoint(x:340,y:250),flyout:openFlyout),"Steep movement below the flyout switches rows")
expect(MenuLayout.aiming(from:NSPoint(x:700,y:480),to:NSPoint(x:680,y:470),flyout:NSRect(x:400,y:300,width:240,height:200)),"Left-opening flyouts use their right edge")
// Key tests report in place; dialogs still close the panel first.
expect(PanelInteraction.keepsPanelOpen(["test-api-key","claude-api"]),"Testing a key keeps the panel open")
expect(!PanelInteraction.keepsPanelOpen(["enter-api-key","claude-api"]),"Typing a key closes the panel for its dialog")
expect(PanelInteraction.keepsPanelOpen(["set-module","claude-api","true"]),"Preferences keep the panel open")
func keyRow(_ key:String)->Entry {try! JSONDecoder().decode(Entry.self,from:Data("{\"label\":\"测试 Key\",\"action\":[\"test-api-key\",\"\(key)\"],\"children\":[]}".utf8))}
expect(PanelInteraction.runningLabel(keyRow("claude-api"),active:["test-api-key","claude-api"])=="正在测试…","The tested key shows progress")
expect(PanelInteraction.runningLabel(keyRow("chatgpt-api"),active:["test-api-key","claude-api"])==nil,"Other keys are not marked as testing")
expect(PanelInteraction.runningLabel(keyRow("claude-api"),active:nil)==nil,"Idle rows keep their label")
// Provider cards: one stats line, normal states stay quiet, notes that change the reading stay visible.
var card=ProviderUsage(id:"claude",name:"Claude",subtitle:"",status:"已同步",available:true,cost:1,tokens:1000,requests:731,output:810000,cache:0.992,coverage:1,sessions:2,unpriced:0,summaryIncluded:true,models:[],history:[])
expect(DashboardView.stats(card)=="731 请求 · 缓存 99.2% · 输出 810.0K · 2 会话","Request, cache, output and session counts share one line")
expect(DashboardView.notes(card).isEmpty,"A synced provider shows no status text")
// Requests the price tables don't know yet say so beside the cost, after what a gateway counted.
var priced=card;priced.subtitle="97 次已由 Magpie 计入";priced.priceNote="75 次未定价";priced.counted=CountedUsage(cost:0,tokens:1,requests:1,unpriced:0,cache:nil,history:[])
expect(DashboardView.notes(priced).map{$0.0}==["97 次已由 Magpie 计入","75 次未定价"] && DashboardView.notes(priced).map{$0.1}==[false,true],"The gateway note stays grey, the price note is highlighted")
expect(ShareCard.tag(priced)=="97 次已由 Magpie 计入" && ShareCard.priceLine(priced)=="75 次未定价，金额只含已定价的部分","Share pictures tag the gateway and give unpriced requests a line of their own")
expect(CardShareCard.blockHeight(priced)==CardShareCard.blockHeight(card)+18 && TerminalShareCard.blockHeight(priced)==TerminalShareCard.blockHeight(card)+17,"That line makes room for itself")
priced.subtitle="";priced.counted=nil;expect(ShareCard.tag(priced)==nil && ShareCard.priceLine(card)==nil,"Nothing to say, nothing drawn")
card.subtitle="未计入 Summary";expect(DashboardView.notes(card).first?.0=="未计入 Summary","A note that changes how to read the totals is shown")
card.status="部分记录未读取";expect(DashboardView.notes(card).first?.1==true,"Problems take the slot and are highlighted")
// Qoder plan requests log Credits only: the card leads with Credits and never shows them as dollars or tokens.
var qoder=ProviderUsage(id:"qoder",name:"Qoder",subtitle:"",status:"已同步",available:true,cost:nil,credits:2.73,tokens:0,requests:3,output:0,cache:nil,coverage:0,sessions:1,unpriced:3,summaryIncluded:true,models:[ModelUsage(name:"qfmodel",tokens:0,share:1,credits:2.73)],history:[])
expect(headline(qoder)=="2.7 Credits" && tokenLine(qoder)=="Token 未记录","Credit-only cards lead with Credits")
expect(DashboardView.stats(qoder)=="3 请求 · 1 会话" && modelAmount(qoder.models[0])=="2.7 Credits","Credit-only stats skip cache and output")
qoder.cost=0.5;qoder.tokens=1200;qoder.output=300;qoder.cache=0.25
expect(headline(qoder)=="$0.50" && DashboardView.stats(qoder).hasSuffix("2.7 Credits"),"Priced tokens lead; Credits move to the stats line")
expect(creditsText(1234.4)=="1234 Credits","Large Credits drop decimals")
// Turning folding back on folds every card at once; replaying saved settings does not.
var folding=PanelInteraction(source:initial)
_ = folding.submit(["set-expanded","claude","true"]);folding.finish(saved(40,["claude"]))
_ = folding.submit(["set-collapse","false"]);folding.finish(PresentationPreferences(revision:41,visible:["summary","opencodex","claude"],appearance:"glass",allowCollapse:false,expandedProviders:["claude"]))
expect(expanded(folding)==Set(["claude"]),"Saved expansions are kept while folding is off")
_ = folding.submit(["set-collapse","true"])
expect(folding.displayed()?.dashboard?.allowCollapse==true && expanded(folding).isEmpty,"Enabling folding collapses every card immediately")
// Light, dark or system: the radio moves at once, and a saved choice survives an older data render.
let themed=try JSONDecoder().decode(Snapshot.self,from:Data(fixture.replacingOccurrences(of:#""moduleOptions":["#,with:#""moduleOptions":[{"label":"跟随系统","checked":true,"radio":true,"action":["set-theme","system"],"children":[]},{"label":"深色","checked":false,"radio":true,"action":["set-theme","dark"],"children":[]},"#).utf8))
var theming=PanelInteraction(source:themed)
expect(themed.theme==nil && PanelInteraction.keepsPanelOpen(["set-theme","dark"]),"Old snapshots carry no theme; choosing one keeps the panel open")
expect(theming.submit(["set-theme","dark"]) == .start,"Choosing dark starts")
func themeChecks(_ s:PanelInteraction)->[Bool]{(s.displayed()?.moduleOptions ?? []).filter{$0.action?.first=="set-theme"}.map{$0.checked ?? false}}
expect(theming.displayed()?.theme=="dark" && themeChecks(theming)==[false,true],"Dark is shown and checked immediately")
var darkSaved=saved(50,[]);darkSaved.theme="dark";theming.finish(darkSaved)
expect(theming.displayed()?.theme=="dark" && themeChecks(theming)==[false,true],"A stale render keeps the saved theme")
var systemSaved=saved(51,[]);systemSaved.theme=nil
_ = theming.submit(["set-theme","system"]);theming.finish(systemSaved)
expect(theming.displayed()?.theme=="system" && themeChecks(theming)==[true,false],"A receipt without a theme means the system one")
// A picture of yesterday is titled and dated as that one day.
let yesterday=try JSONDecoder().decode(Dashboard.self,from:Data(#"{"date":"2026-11-04","dates":[],"providers":[],"rangeKey":"yesterday","rangeLabel":"昨天","chartUnit":"每小时","chartSpan":"11/03","period":["2026-11-03","2026-11-03"],"chartStatus":""}"#.utf8))
expect(ShareCard.title(yesterday)=="昨天 AI 用量","Yesterday's picture says so")
expect(CardShareCard.period(yesterday)=="2026 年 11 月 3 日" && TerminalShareCard.span(yesterday)=="2026-11-03","Yesterday is dated as one day, not today")
// A module row shows its checkbox and then its icon, so its name starts one icon further in than other rows.
let moduleRows=try JSONDecoder().decode([Entry].self,from:Data(#"[{"label":"允许折叠","checked":false,"action":["set-collapse","true"],"children":[]},{"label":"Cursor","checked":true,"image":"PHN2Zy8+","template":true,"action":["set-module","cursor","false"],"children":[]},{"label":"外观与设置","symbol":"slider.horizontal.3","children":[]}]"#.utf8))
expect(moduleRows.map{MenuLayout.left($0,moduleRows)}==[36,56,36],"Only module rows with an icon move their names past it")
let cursorCard=try JSONDecoder().decode(ProviderUsage.self,from:Data(#"{"id":"cursor","name":"Cursor","image":"PHN2Zy8+","template":true,"subtitle":"","status":"","available":true,"tokens":0,"requests":0,"output":0,"sessions":0,"unpriced":0,"models":[],"history":[]}"#.utf8))
expect(cursorCard.template==true && cursorCard.image != nil,"A card carries its tool's logo")
// The module list is a flat grid: the top settings, then one section per title, ticked modules first.
let listing=try JSONDecoder().decode([Entry].self,from:Data(#"[{"label":"允许折叠","detail":"各模块独立展开","checked":false,"action":["set-collapse","true"],"children":[]},{"label":"外观与设置","symbol":"slider.horizontal.3","children":[]},{"label":"","separator":true,"children":[]},{"label":"概览","section":true,"children":[]},{"label":"Summary","checked":true,"symbol":"chart.xyaxis.line","action":["set-module","summary","false"],"children":[]},{"label":"Harness","section":true,"children":[]},{"label":"Cursor","checked":true,"value":"已配置","image":"PHN2Zy8+","template":true,"action":["set-module","cursor","false"],"children":[]},{"label":"Claude","checked":false,"value":"未配置","image":"PHN2Zy8+","action":["set-module","claude","true"],"children":[]}]"#.utf8))
let gemini=try JSONDecoder().decode(Entry.self,from:Data(#"{"label":"Gemini CLI","checked":false,"value":"未配置","image":"PHN2Zy8+","action":["set-module","gemini","true"],"children":[]}"#.utf8))
let grid=ModuleGrid.sections(listing+[gemini])
expect(grid.map{$0.title ?? "-"}==["-","概览","Harness"],"Each title starts a section; separators only part them")
expect(grid[2].cells.map{$0.label}==["Cursor","Claude","Gemini CLI"],"Ticked modules lead their section, the rest keep their order")
let kept=ModuleGrid.sections(listing+[gemini],leading:[gemini.key])[2]
expect(kept.cells.map{$0.label}==["Gemini CLI","Cursor","Claude"],"While the list is open the order it opened with holds")
expect(ModuleGrid.rows(grid[2]).map{$0.count}==[3] && ModuleGrid.rows(ModuleGrid.Section(title:nil,cells:Array(repeating:gemini,count:4))).map{$0.count}==[3,1],"Three to a row")
let expectedHeight:CGFloat=50+72+72
expect(ModuleGrid.height(listing)==expectedHeight,"A row with a note is taller; titles and gaps add up")
expect(ModuleGrid.cellWidth(ModuleGrid.width-16)==188,"Three cells fill the wide list")
// The legend justifies its columns across the full width, three or four per row.
let legendLabels=["总量","OpenCodex","Claude","OpenCode","DeepSeek","WorkBuddy","Gemini CLI"]
let legendRows=DashboardView.legendLayout(legendLabels,width:320)
expect(legendRows.map{$0.count}==[4,3],"Seven short entries fill two balanced rows of four and three")
let starts=legendRows[0].map{$0.minX}
expect(starts[0]==0 && legendRows[0].last!.maxX==320 && abs((starts[1]-starts[0])-(starts[3]-starts[2]))<0.5,"Columns start at the left edge, end at the right edge and are evenly spaced")
for (i,cell) in legendRows[1].enumerated(){expect(cell.minX==legendRows[0][i].minX,"Each legend column starts at the same x on every row")}
expect(DashboardView.legendLayout(Array(legendLabels.prefix(6)),width:320).map{$0.count}==[3,3],"Six entries split three and three")
let wide=DashboardView.legendLayout(["总量","OpenCodex","Claude","OpenCode","Very Long Harness Name","WorkBuddy","Gemini CLI"],width:320)
expect(wide.map{$0.count}==[3,3,1] && wide[0][2].maxX==320 && wide[2][0].minX==0,"A label too long for four columns falls back to three")
let crowded=DashboardView.legendLayout((1...8).map{"Very long harness name \($0)"},width:320)
expect(crowded.allSatisfy{$0.count<=3} && crowded.flatMap{$0}.allSatisfy{$0.minX>=0 && $0.maxX<=320},"Wide labels stay inside the width")
var legendData=grouped;legendData.providers=[("deepseek","DeepSeek Harness"),("codex","ChatGPT"),("claude","Claude App")].map{var p=grouped.providers[0];p.id=$0.0;p.name=$0.1;p.summaryIncluded=true;return p}
expect(DashboardView.legendLabels(legendData)==["总量","DeepSeek","ChatGPT","Claude"],"Legend names drop suffixes the cards already show")
var gateway=grouped.providers[0];gateway.available=true;gateway.clients=nil
let plainHeight=DashboardView.sectionHeight(gateway)
gateway.clients=[ModelUsage(name:"ChatGPT",tokens:60,share:0.6,credits:nil),ModelUsage(name:"Claude Code",tokens:40,share:0.4,credits:nil)]
expect(DashboardView.clientLine(gateway)=="客户端 · ChatGPT 60% · Claude Code 40%" && DashboardView.sectionHeight(gateway)==plainHeight+16,"A gateway card adds one line naming its clients")
gateway.available=false
expect(DashboardView.clientLine(gateway)==nil,"A card without data names no clients")
// The usage heatmap: a week of hours, seven rows of 24, the last 11 hours still to come.
func heat(_ tokens:[Double],columns:Int,future:Int,rows:[String]=[])->UsageHeatmap {
    UsageHeatmap(columns:columns,rowLabels:rows,labels:tokens.indices.map{"第 \($0) 格"},tokens:tokens,requests:tokens.map{$0>0 ? 1 : 0},costs:tokens.map{_ in 0},unpriced:tokens.map{_ in 0},
                 future:future,hint:"每格 1 小时",status:"",notice:"")
}
var week=heat([Double](repeating:0,count:168),columns:24,future:11,rows:["Thu","Fri","Sat","Sun","Mon","Tue","Wed"])
expect(week.rows==7 && week.past==157 && week.note=="暂无用量","Nothing used yet says so under the squares")
for (i,v) in [(100,10.0),(101,20),(102,30),(103,40),(155,5),(156,40)] {week.tokens[i]=v;week.requests[i]=1}
week.requests[60]=3
expect(week.levels[156]==4 && week.levels[103]==4 && week.levels[102]==3 && week.levels[100]==2 && week.levels[155]==1,"Shades go by rank, and the busiest square is the darkest")
expect(week.levels[60]==1 && week.levels[0]==0 && week.note==nil,"Requests without tokens take the lightest shade; idle squares none")
week.costs[156]=0.25;week.unpriced[156]=1;week.requests[156]=2
expect(week.detail(156)=="第 156 格 · 40 Token · 已知 $0.25 · 2 请求" && week.detail(1)=="第 1 格 · 无用量","Pointing at a square reads it out")
week.status="待同步"
expect(week.note=="待同步","Before the first read the line under the squares says so")
week.status="";week.notice="部分小时数据不可用"
expect(DashboardView.heatmapCaption(week,pointed:nil)=="部分小时数据不可用" && DashboardView.heatmapCaption(week,pointed:156)==week.detail(156),"The line under the squares reads out the square pointed at, or what is missing")
week.notice=""
expect(DashboardView.heatmapCaption(week,pointed:nil)==nil,"With nothing to say, the line under the squares holds only the unit")
var withoutHeatmap=grouped;withoutHeatmap.showSummary=true
var heatData=withoutHeatmap;heatData.heatmap=week
let heatGrid=DashboardView.heatmapGrid(heatData,width:344)
expect(abs(heatGrid.minX-12-DashboardView.labelColumn(heatData))<0.01 && abs(heatGrid.maxX-332)<0.01,"The squares start after the shared label column and reach the right edge")
let axisWidth=DashboardView.chartAxis(heatData).labels.map{ceil(($0 as NSString).size(withAttributes:[.font:DashboardView.axisFont]).width)}.max() ?? 0
let rowWidth=(week.rowLabels ?? []).map{ceil(($0 as NSString).size(withAttributes:[.font:DashboardView.rowFont]).width)}.max() ?? 0
expect(DashboardView.labelColumn(heatData)==max(axisWidth,rowWidth)+10,"The chart's axis labels and the heatmap's row names share one column")
var wideRow=heatData;wideRow.heatmap!.rowLabels=["24小时"]
expect(DashboardView.labelColumn(wideRow)>=DashboardView.labelColumn(withoutHeatmap),"A wider row name widens the column for the chart too, so both still start together")
expect(DashboardView.height(heatData)==DashboardView.height(withoutHeatmap)-6+DashboardView.heatmapHeight(heatData),"The heatmap adds its height and takes the gap before the first card")
expect(DashboardView.harnessHeaderHeight(heatData)==0 && DashboardView.harnessHeaderHeight(withoutHeatmap)==6,"The heatmap's hint keeps the first card clear, so no extra gap follows it")
func centre(_ index:Int)->NSPoint {let cell=DashboardView.heatmapCell(heatData,width:344,index:index);return NSPoint(x:cell.midX,y:cell.midY)}
expect(DashboardView.heatmapIndex(at:centre(156),week,DashboardView.heatmapLayout(heatData)!)==156,"The view reads the square from the layout it worked out once")
let square=DashboardView.heatmapCell(heatData,width:344,index:156)
expect(DashboardView.heatmapIndex(at:NSPoint(x:square.maxX+1,y:square.midY),heatData,width:344)==156,"The space beside a square belongs to the nearest one")
expect(DashboardView.heatmapIndex(at:centre(156),heatData,width:344)==156 && DashboardView.heatmapIndex(at:centre(24),heatData,width:344)==24,"Each square is the hour it stands for, a day to a row")
expect(DashboardView.heatmapIndex(at:centre(160),heatData,width:344)==nil && DashboardView.heatmapIndex(at:NSPoint(x:heatGrid.midX,y:heatGrid.maxY+6),heatData,width:344)==nil,"Squares still to come and points outside read nothing")
var hours=heatData;hours.heatmap=heat([Double](repeating:1,count:24),columns:24,future:0)
var days=heatData;days.heatmap=heat([Double](repeating:1,count:30),columns:30,future:0)
let hourSquares=(0..<24).map{DashboardView.heatmapCell(hours,width:344,index:$0)},daySquares=(0..<30).map{DashboardView.heatmapCell(days,width:344,index:$0)}
expect((hourSquares+daySquares).allSatisfy{$0.width==HeatmapLayout.cell && $0.height==HeatmapLayout.cell},"Every range draws the same size of square")
expect(abs(hourSquares.last!.maxX-332)<0.01 && abs(daySquares.last!.maxX-332)<0.01 && hourSquares[1].minX-hourSquares[0].maxX>daySquares[1].minX-daySquares[0].maxX,"Rows of hours and of days both run to the right edge; the space between columns stretches instead")
let weekLayout=DashboardView.heatmapLayout(heatData)!,weekSquares=(0..<48).map{DashboardView.heatmapCell(heatData,width:344,index:$0)}
expect(abs((weekSquares[24].minY-weekSquares[0].maxY)-(weekSquares[1].minX-weekSquares[0].maxX))<0.01 && abs(weekLayout.gap-(weekSquares[1].minX-weekSquares[0].maxX))<0.01,"Rows sit as far apart as columns, so the spacing is even every way")
expect(DashboardView.heatmapHeight(hours)<34 && DashboardView.heatmapHeight(days)<34 && DashboardView.heatmapHeight(heatData)<110,"One row stays short, a week of hours under 110pt")
// A picture taller than a phone fills the screen top to bottom: its content starts below the notch or island,
// with a band of squares in that room, and ends clear of the home indicator.
expect(abs(ShareCard.topBand*393/ShareCard.width-59)<0.5 && ShareCard.bottomBand>=30,"A share picture filling a 393pt screen's width starts its content 59pt down, the island's 11pt above and below, and ends clear of the home indicator")
let patch=NSRect(x:0,y:0,width:100,height:50)
expect(ShareCard.fade(.down,x:0,y:0,in:patch)==1 && ShareCard.fade(.up,x:0,y:50,in:patch)==1 && ShareCard.fade(.left,x:100,y:0,in:patch)==1 && ShareCard.fade(.left,x:0,y:0,in:patch)==0,"A pattern is strongest at the edge it grows from and gone at the far one")
for style in ShareCard.Style.allCases {
    expect(ShareCard.make(style,heatData,project:Project(name:"Starmeter",url:"https://example.com")).frame.height>ShareCard.make(style,withoutHeatmap,project:Project(name:"Starmeter",url:"https://example.com")).frame.height,"The share picture carries the heatmap: "+style.title)
}
heatData.allowCollapse=true
let heatFold=DashboardView(heatData,width:344).subviews.compactMap{$0 as? DisclosureButton}.first!
expect(heatFold.frame.minY==DashboardView.summaryHeight(heatData)+DashboardView.heatmapHeight(heatData)+2,"Folded cards start right below the heatmap")
// The heading ends before the pin, the leftmost header button, 233pt from the right of the 344pt dashboard.
let headingFont=NSFont.systemFont(ofSize:14,weight:.semibold)
for summary in [true,false] {
    var d=withoutHeatmap;d.showSummary=summary
    let w=(DashboardView.heading(d) as NSString).size(withAttributes:[.font:headingFont]).width
    expect(w<=DashboardView.headingWidth(344) && 12+DashboardView.headingWidth(344)<=344-233-6,"The heading fits beside the buttons: "+DashboardView.heading(d))
}
expect(!MenuLayout.aiming(from:NSPoint(x:330,y:480),to:NSPoint(x:332,y:479),flyout:openFlyout),"A tiny drift toward the flyout is not aiming")
// The share picture is opaque, signed, and lists every source that has data.
var shareSource=grouped;shareSource.providers[0].available=false
for style in ShareCard.Style.allCases {
    let card=ShareCard.make(style,shareSource,project:Project(name:"Starmeter",url:"https://github.com/timecatbit/starmeter"))
    expect(card.dashboard.providers.allSatisfy{$0.available} && card.dashboard.providers.count==shareSource.providers.filter{$0.available}.count,"Share \(style) keeps every source with data, as the panel does")
    let picture=card.png().flatMap{NSBitmapImageRep(data:$0)}
    expect(card.bounds.height>200 && picture?.pixelsWide==Int(card.bounds.width*2) && picture?.hasAlpha==true && picture.map{$0.colorAt(x:2,y:2)?.alphaComponent==1} == true,"Share \(style) picture is a 2x opaque PNG")
}
expect(ShareCard.Style.allCases.map{$0.title}==["卡片风格","终端风格"],"People pick the card or the terminal style when they share")
let shareCard=ShareCard.make(.card,shareSource,project:Project(name:"Starmeter",url:"https://github.com/timecatbit/starmeter"))
let clip=shareCard.png().map{ShareCard.pasteboardItem($0)}
expect(clip.map{Set($0.types)==Set([NSPasteboard.PasteboardType.png,.tiff])}==true,"Copied picture carries PNG and TIFF, not a file URL")
expect(PanelInteraction.keepsPanelOpen(["retry-claude-usage"]),"Retrying Claude usage keeps the panel open")
expect(PanelInteraction.keepsPanelOpen(["set-group","model"]),"Choosing how cards group keeps the panel open")
expect(PanelInteraction.keepsPanelOpen(["refresh-live"]) && PanelInteraction.automatic(["refresh-live"]),"A pinned panel's tick keeps it open and is dropped while another action runs")
Currency.unit=MoneyUnit(code:"CNY",symbol:"¥",rate:7,note:nil)
expect(money(2.5)=="¥17.50" && Figure.string("¥17.50",size:16).string=="¥17.50","Estimates read in yuan at the dashboard's rate, the sign set small like the dollar's")
Currency.unit=nil
expect(money(2.5)=="$2.50","Without a rate estimates stay in dollars")
let toolCardJSON=#"{"id":"claude","name":"Claude","subtitle":"","status":"已同步","available":true,"cost":1,"tokens":10,"requests":2,"output":1,"sessions":3,"unpriced":0,"models":[],"history":[10]}"#
let modelCardJSON=#"{"id":"model:claude-opus-5-5","name":"Claude Opus 5.5","accent":"claude","subtitle":"","status":"已同步","available":true,"cost":1,"tokens":10,"requests":2,"output":1,"sessions":0,"unpriced":0,"models":[],"history":[]}"#
let byModel=try JSONDecoder().decode(Dashboard.self,from:Data(("{\"date\":\"2026-11-04\",\"dates\":[\"11/04\"],\"providers\":["+toolCardJSON+"],\"rangeKey\":\"today\",\"rangeLabel\":\"今天\",\"chartUnit\":\"每小时\",\"chartSpan\":\"\",\"chartStatus\":\"\",\"allowCollapse\":true,\"grouping\":\"model\",\"groupings\":[{\"key\":\"tool\",\"label\":\"工具\"},{\"key\":\"model\",\"label\":\"模型\"}],\"cards\":["+modelCardJSON+"]}").utf8))
expect(byModel.cardList.map{$0.name}==["Claude Opus 5.5"] && byModel.providers.map{$0.name}==["Claude"],"Model cards replace the tool cards while Summary keeps the tools")
expect(!byModel.collapsible && byModel.cardList[0].colour=="claude" && byModel.cardList[0].grouped,"Grouped cards never fold and take their maker's colour")
expect(!DashboardView.stats(byModel.cardList[0]).contains("会话") && DashboardView.stats(byModel.providers[0]).contains("3 会话"),"Only tool cards count sessions")
expect(DashboardView.groupingHeight(byModel)==30,"The grouping switch sits above the cards")
// Pages: what used to open beside the panel opens inside it, rows with more under them opening in place.
let pageJSON=#"[{"label":"本周","value":"已用 38%","usageLevel":"normal","detail":"重置于 11/19 22:00","children":[]},{"label":"","separator":true,"children":[]},{"label":"lab","value":"余额 ¥3.02","children":[{"label":"重命名…","action":["rename-api-key","deepseek","x"],"children":[]},{"label":"删除…","action":["remove-api-key","deepseek","x"],"children":[]}]},{"label":"打开用量与账单","symbol":"arrow.up.right","href":"https://example.com","children":[]}]"#
let pageEntries=try JSONDecoder().decode([Entry].self,from:Data(pageJSON.utf8))
expect(DetailStyle.share(pageEntries[0])==0.38 && DetailStyle.share(pageEntries[2])==nil,"A usage row draws its share as a bar; a balance does not")
let closedHeight=DetailView.height(pageEntries,width:344,open:[]),labID="/"+pageEntries[2].key
let openedHeight=DetailView.height(pageEntries,width:344,open:[labID])
expect(openedHeight>closedHeight+50,"Opening a row in place adds its rows below it")
let pageView=DetailView(pageEntries,width:344,open:[])
var heights:[CGFloat]=[];pageView.resized={height,_ in heights.append(height)}
let labRow=pageView.subviews.compactMap{$0 as? DetailRow}.first{$0.entry.label=="lab"}
labRow?.invoke?(labRow!)
expect(pageView.open==[labID] && labRow?.open==true && heights.last==openedHeight,"Clicking a row with more opens it and tells the panel its new height")
labRow?.invoke?(labRow!)
expect(pageView.open.isEmpty && heights.last==closedHeight,"Clicking it again closes it")
var invoked:[String]=[];pageView.invoke={invoked.append($0.label)}
if let link=pageView.subviews.compactMap({$0 as? DetailRow}).first(where:{$0.entry.href != nil}) {link.invoke?(link)}
expect(invoked==["打开用量与账单"],"A row without more runs its action or opens its link")
let aligned=DetailStyle.layout(pageEntries[2],depth:0,width:344,aligned:true).title.minX,plain=DetailStyle.layout(pageEntries[2],depth:0,width:344,aligned:false).title.minX
expect(aligned==plain+DetailStyle.lead,"Rows of a level start their text in one column when any of them has an icon")
// Settings change in place: the panel stays, and the switch or choice shows its new state at once.
expect(PanelInteraction.keepsPanelOpen(["set-currency","CNY"]) && PanelInteraction.keepsPanelOpen(["set-pin-interval","5"]) && PanelInteraction.keepsPanelOpen(["set-service-status","true"]),"A setting without a question keeps the panel open")
expect(!PanelInteraction.keepsPanelOpen(["set-cursor-usage","true"]) && PanelInteraction.keepsPanelOpen(["set-cursor-usage","false"]),"Turning Cursor reading on asks in a dialog; turning it off does not")
expect(!PanelInteraction.keepsPanelOpen(["set-keep-usage","false"]) && PanelInteraction.keepsPanelOpen(["set-keep-usage","true"]),"Dropping kept usage asks first; keeping it does not")
let settingsJSON=#"{"entries":[],"moduleOptions":[{"label":"外观与设置","children":[{"label":"美元（$）","checked":true,"radio":true,"action":["set-currency","USD"],"children":[]},{"label":"人民币（¥）","checked":false,"radio":true,"action":["set-currency","CNY"],"children":[]},{"label":"服务状态","checked":false,"action":["set-service-status","true"],"children":[]}]}]}"#
var choosing=PanelInteraction(source:try JSONDecoder().decode(Snapshot.self,from:Data(settingsJSON.utf8)))
_=choosing.submit(["set-currency","CNY"]);_=choosing.submit(["set-service-status","true"])
let chosenRows=choosing.displayed()?.moduleOptions?.first?.children ?? []
expect(chosenRows.map{$0.checked}==[false,true,true],"A chosen currency and a switched-on setting show at once")
expect(chosenRows[2].action==["set-service-status","false"],"The switch's next click turns it off again")
expect(DetailStyle.toggle(chosenRows[2]) && !DetailStyle.toggle(chosenRows[0]) && DetailSegments.fits(Array(chosenRows[0...1]),width:320),"On/off settings are switches and short choices a segmented control")
let paceJSON=#"[{"label":"本周","value":"已用 38%","usageLevel":"normal","detail":"重置于 11/19 22:00 · 比匀速少用 48%","children":[]},{"label":"5 小时","value":"已用 60%","usageLevel":"normal","detail":"比匀速多用 10%","children":[]},{"label":"Claude 订阅","value":"已用 本周 38% · Fable 79%","usageLevel":"warning","children":[{"label":"x","children":[]}]}]"#
let paceRows=try JSONDecoder().decode([Entry].self,from:Data(paceJSON.utf8))
expect(abs((DetailStyle.pace(paceRows[0]) ?? 0)-0.86)<0.001 && abs((DetailStyle.pace(paceRows[1]) ?? 0)-0.5)<0.001,"The even-pace mark sits at the share used less how far ahead of pace it is")
expect(DetailStyle.share(paceRows[2])==0.79 && MenuLayout.gauge(paceRows[2]),"A homepage row's bar shows its tightest window")
// Pinned: the live chart keeps a reading per refresh, widens from one minute to ten, and starts again on a new day.
let liveBase=try JSONDecoder().decode(Dashboard.self,from:Data(("{\"date\":\"\",\"dates\":[\"11/04\"],\"providers\":["+toolCardJSON+"],\"rangeKey\":\"today\",\"rangeLabel\":\"今天\",\"chartUnit\":\"\",\"chartSpan\":\"\",\"chartStatus\":\"\"}").utf8))
let liveFeed=LiveFeed(),liveNow=Date();liveFeed.started=liveNow.addingTimeInterval(-120)
var liveDay=liveBase
for step in 0..<60 {liveDay.providers[0].tokens=10+Double(step)*1000;liveFeed.add(liveDay,at:liveNow.addingTimeInterval(Double(step*2)-118))}
liveFeed.add(liveDay,at:liveNow.addingTimeInterval(0.2))
expect(liveFeed.samples.count==60,"A redraw of the same reading doesn't add a sample")
expect(abs((liveFeed.rate(liveNow) ?? 0)-30_000)<200,"The rate is the last minute's tokens per minute")
expect(liveFeed.span(liveNow)==120 && liveFeed.span(liveNow.addingTimeInterval(3600))==600 && LiveFeed().span(Date())==60,"The window opens at a minute and widens to ten")
liveDay.providers[0].tokens=5;liveFeed.add(liveDay,at:liveNow.addingTimeInterval(2))
expect(liveFeed.samples.count==1,"A fall (a new day) starts the line again")
expect(LiveChart.nice(27_000_000)==30_000_000 && LiveChart.nice(31_000_000)==40_000_000 && LiveChart.compact(25_000_000)=="25M" && LiveChart.compact(1_500)=="1.5K","The scale tops out at a round value just above the line")
print("Native interaction assertions: \(assertions) passed")
