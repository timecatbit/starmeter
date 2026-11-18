"""Windows tray flyout: a compact dashboard using the shared snapshot contract."""
import base64
import bisect
import math
import re
import sys
import time
from html import escape
from pathlib import Path

from PySide6.QtCore import QPoint, QPointF, QRectF, QSize, Qt, QTimer, Signal
from PySide6.QtGui import (QColor, QFont, QFontMetrics, QIcon, QPainter, QPainterPath,
                           QPalette, QPen, QPixmap)
from PySide6.QtSvg import QSvgRenderer
from shiboken6 import isValid
from PySide6.QtWidgets import (QApplication, QCheckBox, QComboBox, QFrame, QGridLayout, QHBoxLayout, QLabel,
                               QMenu, QScrollArea, QSizePolicy, QToolButton, QVBoxLayout, QWidget)

from host import ROOT, summary_parts

# The logo's drawings (scripts/make_logo.py); in the built app they are bundled under assets.
ASSETS = Path(getattr(sys, '_MEIPASS', ROOT)) / 'assets'

COLORS={'magpie':'#bc860a','opencodex':'#437fc4','claude':'#bd653f','codex':'#a84bca',
        'opencode':'#d4568a','deepseek-harness':'#6555cd','workbuddy':'#ce791c','gemini':'#16979e',
        'qoder':'#329562','cursor':'#56606d'}
TOTAL='#278980'


def tokens(n):
    for unit,scale,precision in [('B',1e9,2),('M',1e6,2),('K',1e3,1)]:
        if n>=scale:return f'{n/scale:.{precision}f}{unit}'
    return f'{n:,.0f}'


# API-equivalent estimates read in dollars, or in yuan at the rate the dashboard carries; set by each render.
CURRENCY={'symbol':'$','rate':1.0}


def cash(n):
    return '—' if n is None else f"{CURRENCY['symbol']}{n*CURRENCY['rate']:,.2f}"


def share(ratio):
    """Whole percents, a decimal near 0 and 100, and a sliver that would round to 0.0% or 100.0% said as such."""
    value=ratio*100
    if 0<value<0.05:return '<0.1%'
    if 99.95<value<100:return '>99.9%'
    return f'{value:.1f}%' if 0<value<1 or 99<value<100 else f'{value:.0f}%'


def credits_text(value):
    """Qoder CN plan requests log Credits and no tokens; Credits are never shown as dollars or tokens."""
    return f'{value:.0f} Credits' if value>=100 else f'{value:.1f} Credits'


def model_name(name):
    """Claude's dated IDs read as the family and version, as on macOS: claude-opus-5-5 → Opus 5.5."""
    if not name.startswith('claude-'):return name
    parts=re.sub(r'-20[0-9]{6}$','',name)[7:].split('-')
    if parts[0] not in ('opus','sonnet','haiku','fable'):return name
    return parts[0].capitalize()+' '+'.'.join(parts[1:])


def headline(p):
    if p.get('cost') is None and p.get('credits') is not None:return credits_text(p['credits'])
    return cash(p.get('cost'))


def token_line(p):
    return 'Token 未记录' if not p['tokens'] and p.get('credits') is not None else tokens(p['tokens'])+' Token'


def card_stats(p):
    """The grey counts after a card's tokens, in the Mac panel's order."""
    return card_stats_forms(p)[0]


def card_stats_forms(p):
    """The counts line, longest first, as the Mac panel picks: round the cache share, then drop output, then the
    sessions, before anything is cut off."""
    if not p['available']:return ['等待可用数据']
    if not p['tokens'] and p.get('credits') is not None:return [f"{p['requests']} 请求 · {p['sessions']} 会话",f"{p['requests']} 请求"]
    requests=f"{p['requests']} 请求";output='输出 '+tokens(p['output'])
    def cache(digits):return ('缓存 %%.%df%%%%'%digits)%(p['cache']*100) if p.get('cache') is not None else '缓存 —'
    extras=[]
    if p['id']=='opencodex':
        if (p.get('coverage') or 1)<1:extras.append('已定价 %.0f%%'%(p['coverage']*100))
    # Model and project cards gather several tools, whose sessions don't add up.
    elif not p['id'].startswith(('model:','project:')):extras.append(f"{p['sessions']} 会话")
    if p.get('cost') is not None and p.get('credits') is not None:extras.append(credits_text(p['credits']))
    forms=[]
    for parts in ([requests,cache(1),output]+extras,[requests,cache(0),output]+extras,[requests,cache(0)]+extras,[requests,cache(0)],[requests]):
        line=' · '.join(parts)
        if line not in forms:forms.append(line)
    return forms


def card_notes(p):
    """Normal states say nothing; a problem, or notes that change how to read the numbers, follow the name: what a
    gateway counted, in grey, then requests the price tables can't price yet, highlighted."""
    status=p.get('status') or ''
    if not p['available']:return [('数据暂不可用' if status in ('运行正常','已同步','') else status,True)]
    if status not in ('运行正常','已同步',''):return [(status,True)]
    return ([(p['subtitle'],False)] if p.get('subtitle') else [])+([(p['serviceNote'],True)] if p.get('serviceNote') else [])\
        +([(p['priceNote'],True)] if p.get('priceNote') else [])


def figure(value,size,weight=650):
    """A figure reads as one number, as on macOS: the digits carry it, the $ smaller, lighter and raised,
    the K/M/B unit smaller."""
    body=value;head=tail=''
    # Qt shrinks superscript on its own, so the $ asks for 0.92 to land near the Mac's 0.62.
    if body[:1] in ('$','¥'):
        head=f'<span style="font-size:{size*.92:.1f}px;font-weight:500;color:{tone().name()};vertical-align:super">{body[0]}</span>';body=body[1:]
    if len(body)>1 and body[-1] in 'KMB' and body[-2].isdigit():
        tail=f'<span style="font-size:{size*.74:.1f}px">{body[-1]}</span>';body=body[:-1]
    item=QLabel(f'<span style="font-size:{size}px;font-weight:{weight}">{head}{escape(body)}{tail}</span>')
    item.setTextFormat(Qt.RichText);item.setAccessibleName(value)
    return item


def axis_tokens(n):
    # Round figures on the chart's scale: 500M, 1.5B, 40K.
    for scale,suffix in ((1e9,'B'),(1e6,'M'),(1e3,'K')):
        if n>=scale:
            value=n/scale
            return (f'{value:.0f}' if value>=10 or value==int(value) else f'{value:.1f}')+suffix
    return f'{n:.0f}'


def chart_axis(dashboard):
    """A rounded ceiling over the highest Summary total, labelled at zero, half and full, as on macOS."""
    parts=summary_parts(dashboard);count=len(dashboard.get('dates',[]))
    totals=[sum(max(0,p['history'][i]) for p in parts if i<len(p['history'])) for i in range(count)]
    observed=max(max(totals,default=0),1);unit=10**math.floor(math.log10(observed))
    ceiling=math.ceil(observed*1.08/unit*2)/2*unit
    return ceiling,[axis_tokens(ceiling*portion) for portion in (0,.5,1)]


def small_font():
    font=QFont(QApplication.font());font.setPointSizeF(8);return font


THEME={'value':'system'}


def use_theme(theme):
    """浅色/深色 from 外观与设置 pin the panel; 跟随系统 hands it back to Windows. Native popups
    follow too where Qt's style can draw both; the panel's own colours come from dark() either way."""
    theme=theme if theme in ('light','dark') else 'system'
    if THEME['value']==theme:return
    THEME['value']=theme
    hints=QApplication.styleHints()
    if hasattr(hints,'setColorScheme'):
        hints.setColorScheme({'light':Qt.ColorScheme.Light,'dark':Qt.ColorScheme.Dark}.get(theme,Qt.ColorScheme.Unknown))


def dark():
    if THEME['value']!='system':return THEME['value']=='dark'
    return QApplication.palette().color(QPalette.Window).lightness()<128


def ink():
    return QColor('#edf0f7' if dark() else '#222835')


def window_color():
    """What shows behind rounded corners: the Windows palette, unless 浅色/深色 says otherwise."""
    color=QApplication.palette().color(QPalette.Window)
    return color if (color.lightness()<128)==dark() else QColor('#202020' if dark() else '#f3f3f3')


def tone(alpha=1.0):
    """Secondary text in the panel's own palette, so painted labels match the styled ones."""
    color=QColor('#a5aebd' if dark() else '#596477');color.setAlphaF(alpha);return color


def heat_active(h):
    requests=h.get('requests',[])
    return [n>0 or (i<len(requests) and requests[i]>0) for i,n in enumerate(h['tokens'])]


def heat_levels(h):
    """Four shades by rank among the squares with tokens, so the busiest is always the darkest and a few
    heavy hours don't wash out the rest. Requests without tokens (Qoder CN Credits) take the lightest."""
    values=sorted(n for n in h['tokens'] if n>0);levels=[]
    for n,active in zip(h['tokens'],heat_active(h)):
        if not active:levels.append(0)
        elif n<=0:levels.append(1)
        else:levels.append(max(1,min(4,math.ceil(4*bisect.bisect_right(values,n)/len(values)))))
    return levels


def heat_detail(h,i):
    """The square pointed at, in one line: when, tokens, known cost and requests."""
    labels=h['labels']
    if not 0<=i<len(labels):return ''
    n=h['requests'][i] if i<len(h['requests']) else 0
    if not heat_active(h)[i]:return labels[i]+' · 无用量'
    parts=[labels[i]]
    if h['tokens'][i]>0:parts.append(tokens(h['tokens'][i])+' Token')
    cost=h['costs'][i] if i<len(h['costs']) else 0
    if cost>0:parts.append(('已知 ' if i<len(h['unpriced']) and h['unpriced'][i]>0 else '')+cash(cost))
    parts.append(f'{n} 请求')
    return ' · '.join(parts)


def heat_note(h):
    """Said at the bottom left while no square is pointed at: why there's nothing yet, or what is missing."""
    if h.get('status'):return h['status']
    if not any(heat_active(h)):return '暂无用量'
    return h.get('notice') or h.get('streak') or ''


def label_column(dashboard):
    """One left column holds the chart's scale and the heatmap's row names, so the plot above and the
    squares below start at the same place and both reach the right edge."""
    metrics=QFontMetrics(small_font())
    words=list((dashboard.get('heatmap') or {}).get('rowLabels') or [])
    if dashboard.get('showSummary',True) and dashboard.get('dates'):words+=chart_axis(dashboard)[1]
    return max((metrics.horizontalAdvance(w) for w in words),default=0)+10


def label(text='',role=''):
    item=QLabel(text);item.setTextFormat(Qt.PlainText)
    if role:item.setObjectName(role)
    return item


def separator():
    line=QFrame();line.setObjectName('divider');line.setFixedHeight(1)
    return line


def tool(text,hint,callback):
    button=QToolButton();button.setText(text);button.setToolTip(hint);button.setAccessibleName(hint)
    button.setAutoRaise(True);button.setMinimumSize(28,26);button.clicked.connect(callback)
    return button


# Pin and PinnedFill in Segoe Fluent Icons (Windows 11) and Segoe MDL2 Assets (Windows 10).
PIN_OFF,PIN_ON='\ue718','\ue841'

LOGO_SIZES=(16,20,24,32,40,48,64,128,256)


def logo_pixmap(size):
    """The app icon at size pixels: its tile without the macOS grid's margin, the 9-dot drawing at 32 and below."""
    renderer=QSvgRenderer(str(ASSETS/('starmeter-small.svg' if size<=32 else 'starmeter.svg')))
    renderer.setViewBox(QRectF(100,100,824,824))
    pix=QPixmap(size,size);pix.fill(Qt.transparent)
    p=QPainter(pix);p.setRenderHint(QPainter.Antialiasing);renderer.render(p);p.end();return pix


def logo():
    """The tray, the windows and the program icon: one drawing for each size Windows asks for."""
    icon=QIcon()
    for size in LOGO_SIZES:icon.addPixmap(logo_pixmap(size))
    return icon


def mark(data):
    """A tool's logo on its card in its own colours (monochrome ones in the text colour); a dot without one."""
    if not data.get('image'):
        dot=label('●');dot.setStyleSheet('color:'+COLORS.get(data.get('accent') or data.get('id'),'#437fc4'))
        dot.setFixedWidth(18);dot.setAlignment(Qt.AlignCenter);return dot
    view=QLabel();view.setPixmap(icon(data).pixmap(16,16));view.setFixedSize(18,18);return view


def icon(entry):
    state=entry.get('statusLight')
    if state:
        pix=QPixmap(16,16);pix.fill(Qt.transparent)
        p=QPainter(pix);p.setRenderHint(QPainter.Antialiasing)
        colors={'ok':'#2b9263','new':'#477bcc','issue':'#cc781d','unknown':'#828a97'}
        color=QColor(colors.get(state,colors['unknown']))
        p.setPen(QPen(color,1.3));p.setBrush(Qt.NoBrush if state=='unknown' else color)
        p.drawEllipse(QRectF(4.5,4.5,7,7));p.end();return QIcon(pix)
    try:
        raw=base64.b64decode(entry.get('image') or '',validate=True)
    except ValueError:return QIcon()
    if not raw or len(raw)>65536:return QIcon()
    pix=QPixmap(16,16);pix.fill(Qt.transparent)
    if not pix.loadFromData(raw):
        renderer=QSvgRenderer(raw)
        if not renderer.isValid():return QIcon()
        p=QPainter(pix);renderer.render(p);p.end()
    if entry.get('template'):
        p=QPainter(pix);p.setCompositionMode(QPainter.CompositionMode_SourceIn)
        p.fillRect(pix.rect(),ink());p.end()
    return QIcon(pix)


class Trend(QWidget):
    def __init__(self,dashboard,axis):
        super().__init__();self.data=dashboard;self.parts=summary_parts(dashboard);self.axis=axis
        self.setFixedHeight(103);self.setAccessibleName('Token 用量趋势')

    def paintEvent(self,_):
        dates=self.data.get('dates',[])
        if not dates:return
        series=[(p.get('counted') or p, COLORS.get(p['id'],'#437fc4'))
                for p in self.data.get('providers',[]) if p.get('available') and p.get('summaryIncluded',True)]
        totals=[sum(p['history'][i] for p in self.parts if i<len(p['history'])) for i in range(len(dates))]
        cap,marks=chart_axis(self.data)
        p=QPainter(self);p.setRenderHint(QPainter.Antialiasing)
        p.setFont(small_font());muted=tone()
        left=self.axis;right=self.width()-1;top=9;bottom=self.height()-19
        for portion,mark in zip((0,.5,1),marks):
            y=bottom-(bottom-top)*portion
            p.setPen(muted);p.drawText(QRectF(0,y-7,left-5,14),Qt.AlignLeft|Qt.AlignVCenter,mark)
            p.setPen(QPen(tone(.4),1,Qt.DotLine))
            p.drawLine(QPointF(left,y),QPointF(right,y))
        positions=self.data.get('chartPositions')
        if not positions or len(positions)!=len(dates):positions=[i/max(1,len(dates)-1) for i in range(len(dates))]
        for values,color,width in [*[(v['history'],c,1.5) for v,c in series],(totals,TOTAL,2.0)]:
            if not values:continue
            path=QPainterPath()
            for i,(at,value) in enumerate(zip(positions,values)):
                pt=QPointF(left+(right-left)*at,bottom-(bottom-top)*value/cap)
                (path.moveTo if i==0 else path.lineTo)(pt)
            p.setPen(QPen(QColor(color),width,Qt.SolidLine,Qt.RoundCap,Qt.RoundJoin))
            if len(values)==1:p.drawPoint(QPointF(left,bottom-(bottom-top)*values[0]/cap))
            else:p.drawPath(path)
        p.setPen(muted)
        for i in sorted({0,len(dates)//2,len(dates)-1}):
            align=Qt.AlignLeft if i==0 else Qt.AlignRight if i==len(dates)-1 else Qt.AlignHCenter
            p.drawText(QRectF(left,bottom+3,right-left,14),align,dates[i])


class Heatmap(QWidget):
    """The squares under Summary, as on macOS: one square size in every range, the gaps stretched so each
    row runs from the label column to the right edge with rows the same gap apart, four shades by rank,
    and the pointed square described at the bottom left rather than in a tooltip."""
    CELL=8;TOP=2;CAPTION=17

    def __init__(self,data,axis):
        super().__init__();self.data=data;self.axis=axis;self.pointed=None
        self.levels=heat_levels(data);self.columns=max(1,data['columns'])
        self.rows=max(1,-(-len(data['tokens'])//self.columns));self.past=max(0,len(data['tokens'])-data.get('future',0))
        # Its height follows its width, which the layout and scroll area settle in one pass; setting it
        # from resizeEvent instead fights the scroll bar appearing and disappearing.
        policy=QSizePolicy(QSizePolicy.Preferred,QSizePolicy.Fixed);policy.setHeightForWidth(True);self.setSizePolicy(policy)
        self.setMouseTracking(True);self.setAccessibleName('用量热力图')

    def geometry_for(self,width):
        size=self.CELL;span=max(size,width-self.axis)
        pitch=(span-size)/(self.columns-1) if self.columns>1 else 0
        gap=pitch-size if self.columns>1 else 1.6
        return size,pitch,size+gap,gap

    def hasHeightForWidth(self):
        return True

    def heightForWidth(self,width):
        _,_,row,gap=self.geometry_for(width)
        return round(self.TOP+self.rows*row-gap+self.CAPTION)

    def sizeHint(self):
        return QSize(330,self.heightForWidth(330))

    def square(self,i):
        size,pitch,row,_=self.geometry_for(self.width())
        return QRectF(self.axis+(i%self.columns)*pitch,self.TOP+(i//self.columns)*row,size,size)

    def index_at(self,point):
        # The nearest square, so the gaps belong to the squares beside them; none for squares still to come.
        size,pitch,row,gap=self.geometry_for(self.width())
        bottom=self.TOP+self.rows*row-gap
        if point.x()<self.axis-1 or point.x()>self.width()+1 or point.y()<self.TOP-1 or point.y()>bottom+1:return None
        column=round((point.x()-self.axis-size/2)/pitch) if pitch else 0
        index=min(self.rows-1,max(0,int((point.y()-self.TOP)/row)))*self.columns+min(self.columns-1,max(0,column))
        return index if index<self.past else None

    def paintEvent(self,_):
        p=QPainter(self);p.setRenderHint(QPainter.Antialiasing);font=small_font();p.setFont(font)
        size,_,row,gap=self.geometry_for(self.width())
        night=dark()
        empty=QColor('#ffffff' if night else '#000000');empty.setAlphaF(.075 if night else .06)
        shades=[empty]+[QColor(TOTAL) for _ in range(4)]
        for shade,alpha in zip(shades[1:],(.3,.52,.76,1)):shade.setAlphaF(alpha)
        p.setPen(tone())
        for r,name in enumerate((self.data.get('rowLabels') or [])[:self.rows]):
            if name:p.drawText(QRectF(0,self.TOP+r*row+size/2-7,self.axis-4,14),Qt.AlignLeft|Qt.AlignVCenter,name)
        for i in range(min(len(self.data['tokens']),self.rows*self.columns)):
            box=self.square(i)
            if i>=self.past:
                p.setPen(QPen(shades[0],.8));p.setBrush(Qt.NoBrush);p.drawRoundedRect(box.adjusted(.4,.4,-.4,-.4),1.6,1.6);continue
            p.setPen(Qt.NoPen);p.setBrush(shades[self.levels[i]]);p.drawRoundedRect(box,1.6,1.6)
            if i==self.pointed:
                ring=ink();ring.setAlphaF(.6)
                p.setPen(QPen(ring,1.2));p.setBrush(Qt.NoBrush);p.drawRoundedRect(box.adjusted(-1,-1,1,1),2.2,2.2)
        line=self.TOP+self.rows*row-gap+3
        hint=QFont(font);hint.setPointSizeF(7.5);p.setFont(hint);p.setPen(tone(.7))
        room=QFontMetrics(hint).horizontalAdvance(self.data['hint'])
        p.drawText(QRectF(self.width()-room-2,line,room+2,13),Qt.AlignRight|Qt.AlignVCenter,self.data['hint'])
        caption=heat_detail(self.data,self.pointed) if self.pointed is not None else heat_note(self.data)
        if caption:
            p.setFont(font);p.setPen(tone());width=max(0,self.width()-self.axis-room-10)
            p.drawText(QRectF(self.axis,line,width,13),Qt.AlignLeft|Qt.AlignVCenter,QFontMetrics(font).elidedText(caption,Qt.ElideRight,width))

    def point(self,index):
        if index!=self.pointed:self.pointed=index;self.update()

    def mouseMoveEvent(self,event):
        self.point(self.index_at(event.position()))

    def leaveEvent(self,event):
        self.point(None);super().leaveEvent(event)


class ShareBar(QWidget):
    """A model's share as a short rounded bar of its own beside the figures, as on macOS, not a rule across the card."""
    WIDTH=44

    def __init__(self,ratio,color):
        super().__init__();self.ratio=max(0.0,min(1.0,ratio));self.color=QColor(color);self.setFixedSize(self.WIDTH,12)

    def paintEvent(self,_):
        p=QPainter(self);p.setRenderHint(QPainter.Antialiasing);p.setPen(Qt.NoPen)
        track=ink();track.setAlphaF(.10 if dark() else .07);p.setBrush(track)
        p.drawRoundedRect(QRectF(0,4,self.WIDTH,4),2,2)
        fill=QColor(self.color);fill.setAlphaF(.85);p.setBrush(fill)
        p.drawRoundedRect(QRectF(0,4,max(4.0,self.WIDTH*self.ratio),4),2,2)


class Elided(QLabel):
    """One line that ends in … when it runs out of room, as the Mac panel truncates, rather than wrapping or clipping."""
    def __init__(self,text,role='',cap=None,forms=None):
        # forms: shorter ways to say the same, tried in order before the last one is cut off.
        super().__init__(text);self.full=text;self.cap=cap;self.forms=forms or [text];self.setTextFormat(Qt.PlainText)
        if role:self.setObjectName(role)
        self.setMinimumWidth(0);self.setSizePolicy(QSizePolicy.Ignored,QSizePolicy.Preferred)

    def minimumSizeHint(self):
        # A zero minimum width counts as unset, so QLabel would report its whole text as the least it needs.
        return QSize(0,super().minimumSizeHint().height())

    def sizeHint(self):
        # Asks for the whole text, so a layout with room gives it back after a narrower moment.
        width=self.fontMetrics().horizontalAdvance(self.full)+2
        return QSize(min(width,self.cap) if self.cap else width,super().sizeHint().height())

    def resizeEvent(self,event):
        super().resizeEvent(event)
        metrics=self.fontMetrics()
        fitting=next((form for form in self.forms if metrics.horizontalAdvance(form)<=self.width()),self.forms[-1])
        shown=metrics.elidedText(fitting,Qt.ElideRight,self.width())
        if shown!=self.text():self.setText(shown)
        if not self.toolTip() or self.toolTip()==self.full:self.setToolTip(self.full if shown!=self.full else '')


class CardHeader(QFrame):
    """A card's first line; with 允许折叠 on, clicking anywhere on it folds or unfolds the card."""
    def __init__(self,toggle):
        super().__init__();self.toggle=toggle
        if toggle:self.setObjectName('row');self.setCursor(Qt.PointingHandCursor)

    def mousePressEvent(self,event):
        if self.toggle and event.button()==Qt.LeftButton:self.toggle()
        else:super().mousePressEvent(event)


class Provider(QWidget):
    """A tool's card as on macOS: logo, name, any note and the cost on the first line; its tokens in bold and
    the counts in grey under the name; each model with a short share bar beside its figures."""
    INDENT=25

    def __init__(self, data, foldable, expanded, act):
        super().__init__();self.setSizePolicy(QSizePolicy.Preferred,QSizePolicy.Maximum)
        box=QVBoxLayout(self);box.setContentsMargins(0,2,0,4);box.setSpacing(3)
        toggle=(lambda:act(['set-expanded',data['id'],'false' if expanded else 'true'])) if foldable else None
        head=CardHeader(toggle);line=QHBoxLayout(head);line.setContentsMargins(0,0,0,0);line.setSpacing(7)
        # A long name (a project folder, a model) ends in … rather than widening the panel.
        title=Elided(data['name'],'cardTitle',cap=200);title.setSizePolicy(QSizePolicy.Preferred,QSizePolicy.Preferred);title.setToolTip(data['name'])
        line.addWidget(mark(data));line.addWidget(title)
        notes=card_notes(data)
        for i,(note,warning) in enumerate(notes):
            # The last note takes what room is left and ends in … if it must; the ones before it stay whole.
            item=Elided(note,'warning' if warning else 'muted')
            if i<len(notes)-1:item.setSizePolicy(QSizePolicy.Preferred,QSizePolicy.Preferred)
            line.addWidget(item,1 if i==len(notes)-1 else 0)
        if not notes:line.addStretch(1)
        line.addWidget(figure(headline(data) if data['available'] else '—',17))
        if foldable:
            arrow=tool('▾' if expanded else '▸',('收起 ' if expanded else '展开 ')+data['name'],toggle)
            arrow.setObjectName('chevron');line.addWidget(arrow)
        box.addWidget(head)
        body=QVBoxLayout();body.setContentsMargins(self.INDENT,0,0,0);body.setSpacing(3);box.addLayout(body)
        if data['available']:
            stats=QHBoxLayout();stats.setSpacing(4);stats.addWidget(label(token_line(data),'cardTokens'))
            # The counts end in … when they run out of room, as on macOS, rather than wrapping mid-word.
            if not foldable or expanded:stats.addWidget(Elided('· '+card_stats(data),'muted',forms=['· '+form for form in card_stats_forms(data)]),1)
            else:stats.addStretch(1)
            body.addLayout(stats)
        else:body.addWidget(Elided(card_stats(data),'muted'))
        if foldable and not expanded:return
        if data['available'] and data.get('clients'):
            clients=label('客户端 · '+' · '.join(f"{c['name']} {share(c['share'])}" for c in data['clients']),'muted')
            clients.setWordWrap(True);body.addWidget(clients)
        # A source with nothing to show yet says so once; no empty model list under it.
        if not data['available']:return
        if not data['models']:
            body.addWidget(label('此时段暂无模型调用','muted'));return
        color=COLORS.get(data.get('accent') or data['id'],'#437fc4')
        models=QGridLayout();models.setContentsMargins(0,5,0,0);models.setHorizontalSpacing(8);models.setVerticalSpacing(2)
        for i,model in enumerate(data['models']):
            name=Elided(model_name(model['name']));name.setToolTip(model['name'])
            if model.get('priceNote'):
                # A model without a price says so after its name, so the card's cost reads as the rest.
                name.setSizePolicy(QSizePolicy.Maximum,QSizePolicy.Preferred)
                cell=QHBoxLayout();cell.setSpacing(5);cell.addWidget(name);cell.addWidget(label(model['priceNote'],'warning'));cell.addStretch(1)
                name=QWidget();name.setLayout(cell);cell.setContentsMargins(0,0,0,0);name.setMinimumWidth(0)
                name.setSizePolicy(QSizePolicy.Ignored,QSizePolicy.Preferred)
            figure_text=credits_text(model['credits']) if not model['tokens'] and model.get('credits') is not None else tokens(model['tokens'])
            amount=label(figure_text);amount.setAlignment(Qt.AlignRight|Qt.AlignVCenter);amount.setMinimumWidth(58)
            ratio=label(share(model['share']),'muted');ratio.setAlignment(Qt.AlignRight|Qt.AlignVCenter);ratio.setMinimumWidth(36)
            models.addWidget(name,i,0);models.addWidget(ShareBar(model['share'],color),i,1)
            models.addWidget(amount,i,2);models.addWidget(ratio,i,3)
        models.setColumnStretch(0,1);body.addLayout(models)


class TrayPanel(QWidget):
    action=Signal(list)
    link=Signal(str)
    shown=Signal(bool)
    share=Signal(str)
    def __init__(self):
        super().__init__(None,Qt.Tool|Qt.FramelessWindowHint|Qt.WindowStaysOnTopHint)
        self.setWindowTitle('Starmeter');self.setFixedWidth(380)
        self.setAttribute(Qt.WA_TranslucentBackground)
        self.surface=QFrame(self);self.surface.setObjectName('surface')
        layout=QVBoxLayout(self);layout.setContentsMargins(0,0,0,0);layout.addWidget(self.surface)
        shell=QVBoxLayout(self.surface);shell.setContentsMargins(1,1,1,1)
        self.scroll=QScrollArea();self.scroll.setWidgetResizable(True);self.scroll.setFrameShape(QFrame.NoFrame)
        self.scroll.setVerticalScrollBarPolicy(Qt.ScrollBarAsNeeded)
        self.scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff);shell.addWidget(self.scroll)
        self.snapshot={};self.manager=False;self.anchor=None;self.menu=None;self.pending=None
        self.hover=QTimer(self,singleShot=True,interval=80)
        self.hover.timeout.connect(self.open_hover)
        self.hovered=None;self.interacting=0;self.menu_refs=[]
        self.dismissed=float('-inf')
        self.pinned=False;self.live=QTimer(self);self.live.timeout.connect(lambda:self.action.emit(['refresh-live']))
        # A copy result shows for two seconds, across rebuilds, like the Mac panel.
        self.copied=None;self.share_button=None;self.share_note=None

    def show_at(self,anchor,manager=False):
        # The ticked modules lead their sections until the list opens again, so ticking never moves cells.
        self.leading=None
        self.anchor=anchor;self.manager=manager;self.render(self.snapshot,force=True)
        self.show();self.raise_();self.activateWindow();self.shown.emit(True)

    def hideEvent(self,event):
        self.cancel_hover();self.pinned=False;self.live.stop()
        if self.menu:self.menu.close()
        if self.pending is not None:self.snapshot=self.pending;self.pending=None
        self.shown.emit(False);super().hideEvent(event)

    def toggle_pin(self):
        self.pinned=not self.pinned
        if self.pinned:
            self.live.start(int(1000*max(1,(self.snapshot or {}).get('pinInterval',2))));self.action.emit(['refresh-live'])
        else:self.live.stop()
        self.render(self.snapshot,force=True)

    def focus_changed(self):
        if self.isVisible() and not self.pinned and not self.interacting and not self.menu and not self.isActiveWindow() and not QApplication.activePopupWidget():
            self.dismissed=time.monotonic();self.hide()

    def just_dismissed(self):
        # Pressing the tray star takes focus from the panel, which closes it before the click arrives;
        # that same click must not open it again.
        return time.monotonic()-self.dismissed<0.5

    def keyPressEvent(self,event):
        if event.key()==Qt.Key_Escape:self.hide()
        else:super().keyPressEvent(event)

    def style(self,appearance,theme='system'):
        use_theme(theme);night=dark()
        # Both backgrounds keep the same contrast with system light and dark text.
        surface=('#242830' if night else '#f8f9fc') if appearance=='glass' else ('#202124' if night else '#ffffff');border='#444953' if night else '#d3dae6'
        text=ink().name();muted='#a5aebd' if night else '#596477'
        hover='#353e4c' if night else '#e8effb'; divider='#404750' if night else '#dce3ed'
        # Held down, a control darkens past its hover tint, as the Mac panel's do.
        pressed='#46516a' if night else '#d3e0f6'
        self.setStyleSheet(f'''
            #surface {{background:{surface};border:1px solid {border};border-radius:13px;}}
            QLabel,QToolButton,QCheckBox,QComboBox {{color:{text};}}
            QScrollArea,#content {{background:transparent;border:0;}}
            QScrollArea > QWidget > QWidget {{background:transparent;}}
            QToolButton {{padding:3px;border-radius:5px;background:transparent;}}
            QToolButton:hover,QCheckBox:hover {{background:{hover};}}
            QToolButton:pressed,QCheckBox:pressed {{background:{pressed};}}
            #pin {{font-family:'Segoe Fluent Icons','Segoe MDL2 Assets';font-size:12px;}}
            #segment {{border:0;border-radius:11px;padding:3px 10px;font-size:12px;color:{muted};background:transparent;}}
            #segment:hover {{background:{hover};}}
            #segment:pressed {{background:{pressed};}}
            #segment:checked {{background:{hover};color:{text};font-weight:650;}}
            QComboBox {{border:1px solid {border};border-radius:5px;padding:4px 8px;min-height:18px;background:{surface};}}
            QComboBox QAbstractItemView {{background:{surface};color:{text};selection-background-color:{hover};selection-color:{text};}}
            #muted {{color:{muted};font-size:11px;}} #divider {{background:{divider};}}
            #warning {{color:{'#e8963a' if night else '#c46a12'};font-size:11px;}}
            #title {{font-weight:700;font-size:19px;}}
            #cardTitle {{font-weight:650;font-size:14px;}} #cardTokens {{font-weight:650;}} #row {{border-radius:6px;}}
            #chevron {{color:{muted};padding:0 2px;min-width:16px;min-height:16px;}}
            #row:hover {{background:{hover};}}
            #section {{font-weight:650;font-size:13px;padding:10px 0 2px 4px;}}
        ''')

    def render(self,snapshot,force=False):
        if not force and not self.isVisible():
            self.snapshot=snapshot;self.pending=None;return
        if not force and (self.menu or self.interacting or QApplication.activePopupWidget()):
            self.pending=snapshot;return
        if not force and snapshot==self.snapshot:return
        self.snapshot=snapshot;self.pending=None;self.cancel_hover()
        offset=self.scroll.verticalScrollBar().value()
        self.style(snapshot.get('appearance','glass'),snapshot.get('theme','system'))
        content=QWidget();content.setObjectName('content')
        box=QVBoxLayout(content);box.setContentsMargins(16,13,16,13);box.setSpacing(8)
        if self.manager:self.modules(box,snapshot)
        else:self.home(box,snapshot)
        box.addStretch()
        self.scroll.setWidget(content)
        if self.anchor is not None:
            screen=QApplication.screenAt(self.anchor) or QApplication.primaryScreen()
            geo=screen.availableGeometry()
            self.setFixedWidth(min(620 if self.manager else 380,geo.width()-12))
            # Wrapped text and the heatmap are as tall as the width makes them.
            natural=content.heightForWidth(self.width()-2) if content.hasHeightForWidth() else -1
            self.setFixedHeight(min(max(natural,content.sizeHint().height())+6,geo.height()-14))
            x=min(geo.right()-self.width()+1,max(geo.left(),self.anchor.x()-self.width()+16))
            # The taskbar is usually at the bottom; availableGeometry also covers top/side bars.
            y=geo.bottom()-self.height()+1 if self.anchor.y()>=geo.center().y() else geo.top()
            self.move(x,y)
        self.scroll.verticalScrollBar().setValue(offset)

    def home(self,box,snapshot,sharing=False):
        d=snapshot.get('dashboard',{});head=QHBoxLayout()
        unit=d.get('currency') or {}
        CURRENCY.update(symbol=unit.get('symbol') or '$',rate=float(unit.get('rate') or 1))
        head.addWidget(label('Summary' if d.get('showSummary',True) else 'Harness 用量','title'))
        head.addStretch()
        if sharing:
            head.addWidget(label(d.get('rangeLabel',''),'muted'))
        else:
            self.share_note=label();head.addWidget(self.share_note)
            share=tool('↗','分享用量图',lambda:self.open_share(share));self.share_button=share
            # Pinned, the panel stays open when it loses focus and re-reads the range on screen every few seconds.
            # The same plain button as its neighbours, in the system icon font: an outline pin, filled when on.
            pin=tool(PIN_ON if self.pinned else PIN_OFF,'取消钉住' if self.pinned else '钉住，每 %d 秒刷新'%snapshot.get('pinInterval',2),self.toggle_pin)
            pin.setObjectName('pin')
            for button in (pin,tool('⌕','自动检测',lambda:self.action.emit(['detect-modules'])),share,
                           tool('⟳','刷新数据',lambda:self.action.emit(['refresh']))):
                head.addWidget(button)
            self.show_copy_state()
            picker=QComboBox();picker.setAccessibleName('统计范围')
            for entry in snapshot.get('rangeOptions',[]):
                picker.addItem(entry['label'],entry['action'][1])
                if entry.get('checked'):picker.setCurrentIndex(picker.count()-1)
            def change_range(i):
                requested=picker.itemData(i)
                shown=picker.findData(d.get('rangeKey'))
                if shown>=0:picker.setCurrentIndex(shown)
                self.action.emit(['set-range',requested])
                QTimer.singleShot(0,self.flush_pending)
            picker.activated.connect(change_range)
            if picker.count():head.addWidget(picker)
        box.addLayout(head)
        if not d:box.addWidget(label('待同步','muted'));return
        if d.get('showSummary',True):
            parts=summary_parts(d);requests=sum(p['requests'] for p in parts)
            priced=any(p.get('cost') is not None for p in parts)
            # "已知" only when a counted source is unavailable or has requests without a price.
            partial=any(not p.get('available') for p in d['providers'] if p.get('summaryIncluded',True)) \
                or any((p.get('unpriced') or 0)>0 for p in parts)
            stats=QHBoxLayout()
            for i,(name,value) in enumerate([('已知 API 估算' if partial else 'API 估算',cash(sum(p.get('cost') or 0 for p in parts) if priced else None)),
                                             ('Token 用量',tokens(sum(p['tokens'] for p in parts)) if parts else '—'),
                                             ('请求',f'{requests:,}' if parts else '—')]):
                col=QVBoxLayout();align=[Qt.AlignLeft,Qt.AlignHCenter,Qt.AlignRight][i]
                col.addWidget(label(name,'muted'),0,align);col.addWidget(figure(value,23,700),0,align)
                stats.addLayout(col,1)
            box.addLayout(stats)
            legends=['<span style="color:'+TOTAL+'">━</span> 总量']
            for p in d['providers']:
                if p.get('summaryIncluded',True) and p['available']:
                    legends.append('<span style="color:'+COLORS.get(p['id'],'#437fc4')+'">━</span> '+escape(p['name']))
            legend=QLabel('　'.join(legends));legend.setWordWrap(True);box.addWidget(legend)
            axis=label_column(d)
            box.addWidget(Trend(d,axis))
            if d.get('heatmap'):box.addWidget(Heatmap(d['heatmap'],axis))
            if note:=[n for n in (d.get('chartStatus'),d.get('updated')) if n]:
                info=label(' · '.join(note),'muted');info.setAlignment(Qt.AlignRight);box.addWidget(info)
        elif d.get('heatmap'):box.addWidget(Heatmap(d['heatmap'],label_column(d)))
        # The cards can be regrouped by model or project; Summary, the chart and the share picture stay per tool.
        cards=None if sharing else d.get('cards')
        options=d.get('groupings') or []
        if not sharing and len(options)>1 and d['providers']:
            row=QHBoxLayout();row.setSpacing(2)
            for option in options:
                button=QToolButton();button.setText(option['label']);button.setCheckable(True);button.setAutoExclusive(True)
                button.setObjectName('segment');button.setChecked(option['key']==d.get('grouping','tool'))
                def choose(_=False,key=option['key']):
                    if key!=d.get('grouping','tool'):self.action.emit(['set-group',key]);QTimer.singleShot(0,self.flush_pending)
                button.clicked.connect(choose);row.addWidget(button)
            row.addStretch(1)
            if d.get('groupNote'):row.addWidget(label(d['groupNote'],'muted'))
            box.addLayout(row)
        if cards is not None and not cards and d['providers']:box.addWidget(label('此时段暂无调用','muted'))
        # Cards are kept apart by space, as on macOS; only the groups below get a rule.
        for p in (d['providers'] if cards is None else cards):
            box.addWidget(Provider(p,not sharing and cards is None and d.get('allowCollapse'),p['id'] in d.get('expandedProviders',[]),self.action.emit))
        if sharing:
            box.addWidget(separator())
            # Signed like the Mac picture: the logo, then the name.
            footer=QHBoxLayout();footer.setSpacing(6)
            mark=QLabel();mark.setPixmap(logo().pixmap(16,16));footer.addWidget(mark)
            footer.addWidget(label('Starmeter  ·  '+d.get('date',''),'muted'));footer.addStretch()
            box.addLayout(footer)
            return
        if d['providers'] and snapshot.get('entries'):box.addWidget(separator())
        for entry in snapshot.get('entries',[]):self.row(box,entry)

    def share_content(self):
        content=QFrame(self);content.setObjectName('surface')
        box=QVBoxLayout(content);box.setContentsMargins(16,13,16,13);box.setSpacing(8)
        self.home(box,self.snapshot,sharing=True)
        content.setFixedWidth(380);content.adjustSize()
        return content

    def modules(self,box,snapshot):
        header=QHBoxLayout();header.addWidget(label('主页模块','title'));header.addStretch()
        header.addWidget(tool('完成','返回主页',self.back));box.addLayout(header)
        visible=snapshot.get('preferences',{}).get('visible',[])
        box.addWidget(label(f'已启用 {len(visible)} 项','muted'))
        # A flat grid like magpie's provider page: plain section titles, every module in view, ticked ones first.
        options=snapshot.get('moduleOptions',[])
        module=lambda e:(e.get('action') or [''])[0]=='set-module'
        if getattr(self,'leading',None) is None:self.leading={tuple(e['action'][:2]) for e in options if module(e) and e.get('checked')}
        sections=[];current=None
        for e in options:
            if e.get('separator'):current=None;continue
            if e.get('section'):current=(e['label'],[]);sections.append(current);continue
            if current is None:current=(None,[]);sections.append(current)
            current[1].append(e)
        for title,cells in sections:
            cells=sorted(cells,key=lambda e:not (module(e) and tuple(e['action'][:2]) in self.leading))
            if title:box.addWidget(label(title,'section'))
            grid=QGridLayout();grid.setHorizontalSpacing(4);grid.setVerticalSpacing(0)
            for i,e in enumerate(cells):
                cell=QWidget();holder=QVBoxLayout(cell);holder.setContentsMargins(0,0,0,0)
                self.row(holder,e,carded=True);grid.addWidget(cell,i//3,i%3)
            for column in range(3):grid.setColumnStretch(column,1)
            box.addLayout(grid)

    def back(self):
        self.manager=False;self.render(self.snapshot,force=True)

    def row(self,box,e,carded=False):
        if e.get('separator'):box.addWidget(separator());return
        if e.get('section'):box.addWidget(label(e['label'],'muted'));return
        row=QFrame();row.setObjectName('row');line=QHBoxLayout(row);line.setContentsMargins(4,0,4,0);line.setSpacing(6)
        if 'checked' in e and e.get('action'):
            check=QCheckBox(e['label']);check.setChecked(e['checked'])
            if e.get('image'):check.setIcon(icon(e))
            if carded:check.setMinimumHeight(28)
            args=list(e['action'])
            if args[0]=='set-module':check.clicked.connect(lambda enabled:self.action.emit([*args[:2],str(enabled).lower()]))
            else:check.clicked.connect(lambda:self.action.emit(args))
            line.addWidget(check,0 if carded else 1)
            if carded:
                # A configured module carries a green dot after its name, as magpie marks connected ones.
                if e.get('value')=='已配置':dot=label('●');dot.setStyleSheet('color:#2b9263;font-size:9px;');line.addWidget(dot)
                line.addStretch(1)
            hover_target=check
        else:
            button=tool(e['label'],e['label'],lambda:self.activate(e,row));button.setSizePolicy(QSizePolicy.Expanding,QSizePolicy.Fixed)
            button.setStyleSheet('text-align:left;');button.setToolButtonStyle(Qt.ToolButtonTextBesideIcon)
            button.setIcon(icon(e));line.addWidget(button,1);hover_target=button
        if (value:=e.get('value')) and not (carded and value in ('已配置','未配置')):
            readout=label(('● ' if value=='已配置' else '○ ' if value=='未配置' else '')+value,'muted')
            if e.get('attention'):readout.setStyleSheet('font-weight:600;color:#477bcc')
            if e.get('usageLevel') in ('warning','critical'):
                readout.setStyleSheet('color:'+('#cc781d' if e['usageLevel']=='warning' else '#cc4b52'))
            line.addWidget(readout)
        if e.get('children'):
            arrow=tool('›','查看 '+e['label']+' 详情',lambda:self.open_menu(e,row));line.addWidget(arrow)
            # Checkboxes must be usable for repeated toggles without firing a side menu.
            for target in [arrow]+([] if 'checked' in e else [hover_target]):
                target.enterEvent=lambda event,e=e,row=row:self.queue_menu(e,row)
                target.leaveEvent=lambda event:self.cancel_hover()
        box.addWidget(row)

    def queue_menu(self,entry,row):
        if self.menu:return
        self.hovered=(entry,row);self.hover.start()

    def cancel_hover(self):
        self.hover.stop();self.hovered=None

    def open_hover(self):
        if self.hovered:self.open_menu(*self.hovered)

    def activate(self,entry,row):
        if entry.get('children'):self.open_menu(entry,row)
        elif entry.get('action'):self.action.emit(entry['action'])
        elif entry.get('href'):self.link.emit(entry['href'])

    def open_menu(self,entry,row):
        self.cancel_hover()
        if self.menu:return
        menu=QMenu(self);menu.setSeparatorsCollapsible(True);self.menu=menu
        self.populate_menu(menu,entry['children'])
        menu.aboutToHide.connect(lambda:self.menu_closed(menu))
        hint=menu.sizeHint();pos=row.mapToGlobal(QPoint(0,0));geo=(QApplication.screenAt(pos) or QApplication.primaryScreen()).availableGeometry()
        x=self.geometry().right()+2
        if x+hint.width()>geo.right():x=self.geometry().left()-hint.width()
        menu.popup(QPoint(max(geo.left(),x),max(geo.top(),min(pos.y(),geo.bottom()-hint.height()+1))))
        if entry.get('onOpenAction'):self.action.emit(entry['onOpenAction'])

    def open_share(self,button):
        self.cancel_hover()
        if self.menu:return
        menu=QMenu(self);self.menu=menu
        for text,mode in (('复制图片','copy'),('保存图片…','save')):
            menu.addAction(text,lambda mode=mode:QTimer.singleShot(0,lambda:self.share.emit(mode)))
        menu.aboutToHide.connect(lambda:self.menu_closed(menu))
        menu.popup(button.mapToGlobal(QPoint(0,button.height())))

    def copied_result(self,ok):
        self.copied=(ok,time.monotonic()+2);self.show_copy_state()
        QTimer.singleShot(2050,self.show_copy_state)

    def show_copy_state(self):
        if self.copied and time.monotonic()>=self.copied[1]:self.copied=None
        button,note=self.share_button,self.share_note
        if not (button and note and isValid(button) and isValid(note)):return
        ok=None if self.copied is None else self.copied[0]
        color='' if ok is None else '#2b9263' if ok else '#cc781d'
        button.setText('↗' if ok is None else '✓' if ok else '!')
        note.setText('' if ok is None else '已复制' if ok else '复制失败')
        for widget in (button,note):widget.setStyleSheet('color:'+color+';' if color else '')

    def populate_menu(self,menu,entries):
        for e in entries:
            if e.get('separator'):menu.addSeparator();continue
            heading=(e['label']+('    '+e['value'] if e.get('value') else '')).replace('&','&&')
            if e.get('children'):
                sub=QMenu(heading,menu);sub.setIcon(icon(e));menu.addMenu(sub)
                self.menu_refs.append(sub);self.populate_menu(sub,e['children']);continue
            action=menu.addAction(icon(e),heading)
            if 'checked' in e:action.setCheckable(True);action.setChecked(e['checked'])
            if 'detail' in e:
                # Keep the explanation visible in the flyout without making it an active row.
                for text in e['detail'].splitlines():
                    for start in range(0,len(text),55):
                        detail=menu.addAction('    '+text[start:start+55].replace('&','&&'));detail.setEnabled(False)
            if e.get('action'):action.triggered.connect(lambda _checked=False,args=e['action']:QTimer.singleShot(0,lambda:self.action.emit(args)))
            elif e.get('href'):action.triggered.connect(lambda _checked=False,url=e['href']:QTimer.singleShot(0,lambda:self.link.emit(url)))
            else:action.setEnabled(False)

    def menu_closed(self,menu):
        if self.menu is menu:self.menu=None;self.menu_refs=[]
        menu.deleteLater()
        QTimer.singleShot(0,self.flush_pending)

    def flush_pending(self):
        if self.pending is not None:self.render(self.pending)
