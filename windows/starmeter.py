#!/usr/bin/env python3
"""Standalone Starmeter tray host. SwiftBar and its scripts aren't involved."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading

from PySide6.QtCore import QLockFile, QPoint, QTimer, QUrl, Signal, QObject
from PySide6.QtGui import QCursor, QDesktopServices, QImage
from PySide6.QtWidgets import (QApplication, QFileDialog, QInputDialog, QLineEdit, QMenu,
                               QMessageBox, QSystemTrayIcon)

import engine
import host
from panel import TrayPanel, logo, window_color

REPOSITORY='https://github.com/timecatbit/starmeter'


class Desktop(QObject):
    ready=Signal(object,int)
    message=Signal(str,str)
    wake=Signal(object)
    command_done=Signal(object)
    # A question from an engine run on the worker thread, answered on the GUI thread.
    gui_call=Signal(object)

    def __init__(self, app, state, version):
        super().__init__()
        self.app=app;self.state=state;self.version=version
        self.windows={}
        self.gui_thread=threading.get_ident()
        self.pool=ThreadPoolExecutor(max_workers=1,thread_name_prefix='starmeter')
        self.visible=False;self.running=False;self.pending=False;self.revision=0
        self.initialized=False;self.updating=set();self.exiting=False;self.reported_failure=False
        self.panel=TrayPanel();self.panel.action.connect(self.action);self.panel.link.connect(self.open_link)
        self.panel.shown.connect(self.visibility);self.panel.share.connect(self.share)
        self.ready.connect(self.publish);self.message.connect(self.show_notice);self.wake.connect(self.request_refresh)
        self.command_done.connect(self.finish_command);self.gui_call.connect(self.run_gui_call)
        self.tray=QSystemTrayIcon(logo(),self);self.tray.setToolTip('Starmeter')
        self.tray.activated.connect(self.tray_action)
        self.tray.messageClicked.connect(lambda:self.open_panel())
        self.menu=QMenu()
        self.menu.addAction('主页模块…',lambda:self.open_panel(manager=True))
        self.menu.addAction('自动检测…',lambda:self.action(['detect-modules']))
        self.menu.addAction('关于 Starmeter '+version,lambda:self.open_link(REPOSITORY+'/releases'))
        self.menu.addSeparator()
        self.startup=self.menu.addAction('登录 Windows 时启动');self.startup.setCheckable(True)
        self.startup.triggered.connect(self.set_autostart)
        self.menu.aboutToShow.connect(self.before_tray_menu)
        self.menu.addAction('退出',self.exit_requested)
        self.tray.setContextMenu(self.menu)
        self.timer=QTimer(self,interval=60_000);self.timer.timeout.connect(self.request_refresh)
        app.focusWindowChanged.connect(lambda _:QTimer.singleShot(0,self.panel.focus_changed))
        app.paletteChanged.connect(lambda _:self.panel.render(self.panel.snapshot,force=True) if self.visible else None)
        app.aboutToQuit.connect(self.quit)

    def before_tray_menu(self):
        if self.panel.isVisible():self.panel.hide()
        try:self.startup.setChecked(host.autostart())
        except OSError:self.startup.setEnabled(False)

    def set_autostart(self,value):
        try:host.autostart(value)
        except OSError:
            self.startup.setChecked(not value);self.error('无法保存启动设置','请检查 Windows 是否允许修改当前用户的启动项。')

    def start(self,background):
        self.tray.show();self.timer.start()
        if not background:self.open_panel()
        self.request_refresh()

    def exit_requested(self):
        if self.updating:
            for key in self.updating:self.windows[key].showNormal()
            self.error('CLI 正在更新','更新完成后再退出 Starmeter。')
            return
        self.app.quit()

    def quit(self):
        self.exiting=True;self.timer.stop();self.tray.hide()
        self.pool.shutdown(wait=False,cancel_futures=True)

    def tray_action(self,reason):
        if reason==QSystemTrayIcon.Trigger:
            if self.panel.isVisible():self.panel.hide()
            elif not self.panel.just_dismissed():self.open_panel()

    def open_panel(self,manager=False):
        geometry=self.tray.geometry()
        self.panel.show_at(geometry.center() if geometry.isValid() else QCursor.pos(),manager)

    def visibility(self,visible):
        self.visible=visible
        # The engine reads sources more often while the panel is open; it learns that from here.
        os.environ['OCX_PANEL_VISIBLE']='1' if visible else '0'
        if visible:self.request_refresh()

    # The engine: one run per request. Its questions for the user are answered here, on the GUI thread.
    def engine(self,*arguments):
        return engine.run(list(arguments),self.answer)

    def on_gui(self,call):
        if threading.get_ident()==self.gui_thread:return call()
        box={};done=threading.Event()
        self.gui_call.emit((call,box,done))
        while not done.wait(0.2):
            if self.exiting:return None
        if 'error' in box:raise box['error']
        return box.get('value')

    def run_gui_call(self,item):
        call,box,done=item
        try:box['value']=call()
        except Exception as error:box['error']=error
        finally:done.set()

    def answer(self,message):
        kind=message.get('host');text=lambda key,default='':str(message.get(key) or default)
        # Notices, refreshes and settings files need no window, so a background run doesn't wait for the GUI.
        if kind=='notify':return self.notice(text('subtitle'),text('message'))
        if kind=='refresh':self.request_refresh(message.get('mode') or 'engine');return True
        if kind=='submitTest':self.submit_test(text('provider'),text('id','default'));return True
        if kind=='saveOverrides':
            values=message.get('values')
            if not isinstance(values,dict):return False
            self.save_overrides(values);return True
        windowed={
            'confirm':lambda:self.confirm(text('message'),text('detail'),text('accept','确定')),
            'text':lambda:self.text_dialog(text('message'),text('value'),bool(message.get('hidden')),text('title','连接设置')),
            'choose':lambda:self.choose_region(text('title'),text('prompt'),*[tuple(option) for option in message.get('options') or []][:2]),
            'choosePath':lambda:self.choose_path(text('message'),bool(message.get('folder'))),
            'savePath':lambda:self.save_path(text('message'),text('name')),
            'error':lambda:self.error(text('title'),text('detail')),
            'openLocal':lambda:self.open_local(text('path'),bool(message.get('text'))) or True,
            'openClaude':lambda:self.open_claude() or True,
            'copy':lambda:self.copy(text('text')) or True,
            'updateCLI':lambda:self.update_cli(text('binary'),text('key')) or True}
        return self.on_gui(windowed[kind]) if kind in windowed else None

    def request_refresh(self,mode=None):
        if threading.get_ident()!=self.gui_thread:self.wake.emit(mode);return
        if self.exiting:return
        if self.running:
            if mode or self.visible:self.pending=True
            return
        self.running=True;self.pool.submit(self.refresh,self.revision)

    def refresh(self,revision):
        snapshot=None
        try:
            # Paint saved data first. Fresh results arrive as one snapshot with their range.
            snapshot=self.engine('snapshot' if self.initialized else 'prepare');self.initialized=True
            self.ready.emit(snapshot,revision)
            snapshot=self.engine('refresh')
        except Exception:
            host.log_failure(self.state,'refresh')
            if snapshot is None and not self.reported_failure:
                self.reported_failure=True
                self.message.emit('本机数据读取失败','可从托盘菜单打开设置，或稍后重试。')
        else:self.reported_failure=False
        finally:
            self.ready.emit((snapshot,'done'),revision)

    def publish(self,value,revision):
        done=isinstance(value,tuple)
        if done:value=value[0]
        if revision==self.revision and value is not None:
            value=dict(value,entries=host.filter_entries(value['entries']),moduleOptions=host.filter_entries(value['moduleOptions']))
            self.panel.render(value)
            # A tray icon can't carry text: the figure chosen for beside the star goes in its tooltip.
            self.tray.setToolTip('Starmeter'+(' · '+value['menuBarText'] if value.get('menuBarText') else ''))
        if done:
            self.running=False
            if self.pending and not self.exiting:self.pending=False;self.request_refresh()

    def action(self,args):
        if not args or args[0] not in host.PORTABLE_ACTIONS:return
        name,*extra=args
        if name=='panel-modules':self.open_panel(True);return
        if name=='test-api-key':self.submit_test(*extra);return
        # Network work and the actions that only ask before it run off the GUI thread; their questions come back to it.
        background={'check-update','account-pause','pool-strategy','auto-switch','failover','sync','account-use','update',
                    'refresh','refresh-source','refresh-monitor','check-app-updates','detect-modules'}
        if name=='refresh-view':self.request_refresh();return
        # A pinned panel's tick: dropped while a refresh is still running, so ticks never pile up.
        if name=='refresh-live':
            if not self.running:self.running=True;self.pool.submit(self.live,self.revision)
            return
        # A visible selection must never be reverted by a refresh already in flight.
        if name not in ('refresh-view','refresh-source','refresh','refresh-monitor','check-app-updates'):
            self.revision+=1
        if name in background:self.run_background(self.engine,'action',name,*extra);return
        try:
            self.panel.interacting+=1
            self.engine('action',name,*extra)
        except (OSError,ValueError,RuntimeError):
            host.log_failure(self.state,'action '+name)
            self.error('操作未完成','请检查本机路径或连接设置后重试。')
        finally:
            self.panel.interacting-=1
        self.request_refresh('action')
        self.panel.flush_pending()

    def live(self,revision):
        """A pinned panel's tick: the range on screen and the sessions read again from the local logs."""
        snapshot=None
        try:snapshot=self.engine('live')
        except Exception:host.log_failure(self.state,'live')
        finally:self.ready.emit((snapshot,'done'),revision)

    def run_background(self,func,*args,done=None):
        future=self.pool.submit(func,*args)
        future.add_done_callback(lambda result:self.command_done.emit((result,done)))

    def finish_command(self,value):
        if self.exiting:return
        future,done=value
        try:result=future.result()
        except Exception:
            host.log_failure(self.state,'command')
            self.error('操作未完成','请检查本机设置或稍后重试。')
        else:
            if done is not None:done(result)
        self.request_refresh('command')

    def submit_test(self,*args):
        self.pool.submit(self.engine,'action','test-api-key',*args)

    def open_link(self,url):
        if host.web_link(url):
            self.panel.hide();QDesktopServices.openUrl(QUrl(url))

    def open_local(self,path,text=False):
        if not Path(path).exists():raise FileNotFoundError(path)
        if text and os.name=='nt':
            subprocess.Popen(['notepad.exe',path],creationflags=subprocess.CREATE_NO_WINDOW)
        elif not QDesktopServices.openUrl(QUrl.fromLocalFile(path)):raise OSError('open failed')

    def open_claude(self):
        local=Path(os.environ.get('LOCALAPPDATA',''))/'AnthropicClaude'/'claude.exe'
        if local.is_file():self.open_local(str(local))
        else:self.open_link('https://claude.ai/')

    def config_file(self):
        path=self.state/'config.local.json'
        if not path.exists():host.write_json(path,{})
        return path

    def save_overrides(self,values):
        # The engine explicitly clears classic values when switching to key files.
        changes={k:v for k,v in values.items() if k in host.OVERRIDE_NAMES or v=='' and k in host.KEYS}
        if len(changes)!=len(values) or any(not isinstance(v,str) or '\n' in v or '\x00' in v for v in changes.values()):
            raise ValueError('invalid config')
        path=self.config_file()
        payload=dict(host.overrides(path),**changes)
        host.write_json(path,payload)
        if host.read_json(path)!=payload:raise OSError('settings could not be saved')
        # Later engine runs start with the new connection.
        os.environ.update(changes)

    def choose_path(self,message,folder):
        start=str(Path.home())
        if folder:return QFileDialog.getExistingDirectory(self.panel,message,start) or None
        return QFileDialog.getOpenFileName(self.panel,message,start)[0] or None

    def save_path(self,message,name):
        kind=Path(name).suffix.lstrip('.')
        path,_=QFileDialog.getSaveFileName(self.panel,message,str(Path.home()/name),'%s 文件 (*.%s)'%(kind.upper(),kind))
        return path or None

    def text_dialog(self,message,default,hidden,title):
        text,ok=QInputDialog.getText(self.panel,title,message,QLineEdit.Password if hidden else QLineEdit.Normal,default)
        return text.strip() if ok else None

    def choose_region(self,title,prompt,first,second):
        label,ok=QInputDialog.getItem(self.panel,title,prompt,[first[0],second[0]],0,False)
        return {first[0]:first[1],second[0]:second[1]}.get(label) if ok else None

    def confirm(self,title,detail,accept):
        box=QMessageBox(QMessageBox.Question,'Starmeter',title,parent=self.panel)
        from PySide6.QtCore import Qt
        box.setTextFormat(Qt.PlainText)
        box.setInformativeText(detail)
        ok=box.addButton(accept,QMessageBox.AcceptRole)
        cancel=box.addButton('取消',QMessageBox.RejectRole);box.setDefaultButton(cancel)
        self.panel.interacting+=1
        try:box.exec()
        finally:self.panel.interacting-=1
        return box.clickedButton()==ok

    def error(self,title,detail):
        self.panel.interacting+=1
        try:QMessageBox.warning(self.panel,title,detail)
        finally:self.panel.interacting-=1

    def copy(self,text):
        self.app.clipboard().setText(text)

    def notice(self,subtitle,body):
        if not self.exiting and QSystemTrayIcon.supportsMessages():
            self.message.emit(subtitle,body);return True
        return False

    def show_notice(self,subtitle,body):
        if self.tray.isVisible():
            self.tray.showMessage('Starmeter'+(' · '+subtitle if subtitle else ''),body,self.tray.icon())

    def share_image(self):
        # A separate view captures the full scrollable content, not the screen behind the panel.
        if not self.panel.snapshot.get('dashboard'):return None
        clone=TrayPanel();clone.snapshot=self.panel.snapshot;clone.style(clone.snapshot.get('appearance','glass'),clone.snapshot.get('theme','system'))
        content=clone.share_content()
        ratio=max(1,self.panel.devicePixelRatio())
        pic=QImage(round(content.width()*ratio),round(content.height()*ratio),QImage.Format_RGB32)
        pic.setDevicePixelRatio(ratio);pic.fill(window_color())
        content.render(pic);clone.deleteLater()
        return pic

    def share(self,mode='save'):
        pic=self.share_image()
        if pic is None:return
        if mode=='copy':
            # Only the image goes on the clipboard, no file path, so it pastes straight into a chat.
            clipboard=self.app.clipboard();clipboard.setImage(pic)
            self.panel.copied_result(clipboard.mimeData().hasImage());return
        self.panel.interacting+=1
        try:path,_=QFileDialog.getSaveFileName(self.panel,'保存用量图','Starmeter.png','PNG 图片 (*.png)')
        finally:self.panel.interacting-=1
        self.panel.flush_pending()
        if path:
            if not path.lower().endswith('.png'):path+='.png'
            if not pic.save(path):self.error('保存失败','请选择可写入的文件夹。')

    def update_cli(self,binary,key):
        if key in self.updating:
            self.windows[key].showNormal();self.windows[key].raise_();return
        from update_window import UpdateWindow
        window=UpdateWindow(binary,key,self.state);self.updating.add(key)
        window.completed.connect(lambda:self.finish_update(key))
        window.setAttribute(window.delete_on_close)
        window.show()
        # Keep a Python reference while the process and its log are active.
        self.windows[key]=window
        window.destroyed.connect(lambda _=None,w=window:self.windows.pop(key,None) if self.windows.get(key) is w else None)

    def finish_update(self,key):
        self.updating.discard(key);self.action(['check-app-updates'])


def single_instance(state):
    """The held per-user lock, or None while another copy runs."""
    lock=QLockFile(str(state/'tray.lock'));lock.setStaleLockTime(0)
    # PySide6 6.8 needs the timeout: Qt moved the default onto its chrono overload, which isn't bound.
    return lock if lock.tryLock(0) else None


def self_test(app,state,render_path=None):
    """Packaged smoke test with synthetic local history, no network, tray or account access. It runs the bundled
    engine end to end, its questions to the tray included."""
    import json
    import time
    from panel import Trend
    root=state/'fixture';root.mkdir(parents=True)
    (root/'usage.jsonl').write_text(json.dumps({'t':time.time()-3600,'agent':'codex','provider':'test',
                                                'model':'gpt-5.4','in':1000,'out':10})+'\n',encoding='utf-8')
    asked=[]
    def answer(message):
        asked.append(message.get('host'))
        return str(root) if message.get('host')=='choosePath' else True
    engine.run(['select','summary','heatmap','magpie'],answer)
    # The folder comes back through the same question a user would answer in a file dialog.
    engine.run(['action','choose-source','magpie'],answer)
    engine.run(['action','set-range','7d'],answer)
    assert 'choosePath' in asked and host.read_json(state/'view-range.json').get('range')=='7d'
    snapshot=engine.run(['range'],answer)
    assert snapshot['dashboard']['rangeKey']=='7d'
    assert snapshot['dashboard']['providers'][0]['tokens']==1010
    # The engine found its logos and price tables beside it.
    assert snapshot['dashboard']['providers'][0].get('image'),'the engine shipped without its logos'
    assert snapshot['dashboard']['heatmap'] and sum(snapshot['dashboard']['heatmap']['tokens'])==1010
    view=TrayPanel();view.snapshot=snapshot;view.show_at(QPoint(800,780));app.processEvents()
    assert view.isVisible() and view.scroll.widget().findChild(Trend)
    if render_path and not view.grab().save(render_path):raise OSError('could not write preview')
    view.hide()
    # The same start-up the tray runs, short of the message loop.
    lock=single_instance(state);assert lock is not None and single_instance(state) is None
    desktop=Desktop(app,state,engine.run(['version'],answer));desktop.start(True)
    deadline=time.monotonic()+30
    while (desktop.running or desktop.pending) and time.monotonic()<deadline:
        app.processEvents();time.sleep(0.02)
    assert not desktop.running and desktop.panel.snapshot.get('dashboard')
    assert not desktop.share_image().isNull()
    desktop.tray_action(QSystemTrayIcon.Trigger);app.processEvents()
    assert desktop.panel.isVisible()
    if os.name=='nt':desktop.before_tray_menu();assert not desktop.panel.isVisible()
    # Opening the panel queued another refresh; let it finish writing into the temporary state before
    # that directory is removed, or the removal races it ("Directory not empty").
    desktop.quit();desktop.pool.shutdown(wait=True);lock.unlock()
    if sys.stdout is not None:print('Starmeter smoke test OK',flush=True)


def main():
    args=argparse.ArgumentParser()
    args.add_argument('--background',action='store_true')
    args.add_argument('--self-test',action='store_true')
    args.add_argument('--preview')
    options=args.parse_args()
    if os.name=='nt':
        import ctypes
        ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID('timecatbit.TokenMonitor')
    app=QApplication(sys.argv[:1]);app.setQuitOnLastWindowClosed(False);app.setApplicationName('Starmeter');app.setWindowIcon(logo())
    if os.name=='nt':
        # The text is Simplified Chinese. Without a named CJK face, Windows outside a Chinese locale falls
        # back to a Japanese font, which lacks forms like 请 and 时 and draws the rest in Japanese shapes.
        font=app.font();font.setFamilies(['Segoe UI','Microsoft YaHei UI','Microsoft YaHei']);app.setFont(font)
    if options.self_test:
        with tempfile.TemporaryDirectory(prefix='starmeter-smoke-') as temp:
            state=Path(temp)
            for key in list(os.environ):
                if key.startswith(('OCX_','MAGPIE_','KOMARI_','CODEX_','CLAUDE_')):os.environ.pop(key)
            os.environ.update(OCX_SELF_UPDATES='0',OCX_PRICING_UPDATES='0')
            host.prepare_environment(state)
            self_test(app,state,options.preview)
        return 0
    if os.name!='nt':
        QMessageBox.warning(None,'Starmeter','这是 Windows 托盘版。macOS 请使用 SwiftBar 版本。');return 2
    state=host.state_dir();state.mkdir(parents=True,exist_ok=True);host.prepare_environment(state)
    lock=single_instance(state)
    if lock is None:
        if not options.background:QMessageBox.information(None,'Starmeter','已在运行，请点任务栏右下角的星星。')
        return 0
    # The installer and uninstaller look for this name (AppMutex) and ask to quit the tray first.
    # Windows releases the handle when the process exits.
    ctypes.windll.kernel32.CreateMutexW(None,False,'TokenMonitorTray')
    if not QSystemTrayIcon.isSystemTrayAvailable():
        QMessageBox.warning(None,'Starmeter','未找到系统托盘，请登录 Windows 桌面后重试。');return 2
    try:version=engine.run(['version'],lambda message:None,timeout=30)
    except (OSError,engine.EngineError):
        QMessageBox.warning(None,'Starmeter','程序文件不完整，请重新安装 Starmeter。');return 2
    desktop=Desktop(app,state,version);desktop.start(options.background)
    return app.exec()


if __name__=='__main__':
    sys.exit(main())
