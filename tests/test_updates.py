import json,os,tempfile,time,types,unittest
from pathlib import Path
from unittest.mock import MagicMock, patch
SOURCE=Path(__file__).resolve().parents[1]/'starmeter.30s.sh'
CODE=(SOURCE.parent/'lib/swiftbar_plugin.py').read_text()
class UpdateTests(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
  env=patch.dict(os.environ,{'KOMARI_BASE_URL':'https://komari.example.test','OS_APPEARANCE':'Dark','OCX_CACHE_DIR':self.tmp.name,'SWIFTBAR_PLUGIN_DATA_PATH':self.tmp.name,'OCX_PLUGIN_PATH':str(SOURCE.resolve())});env.start();self.addCleanup(env.stop)
  self.m=types.ModuleType('updates_test');exec(compile(CODE,str(SOURCE.parent/'lib/swiftbar_plugin.py'),'exec'),self.m.__dict__)
  configured=patch.object(self.m,'ocx_configured',return_value=False);configured.start();self.addCleanup(configured.stop)
  installed=patch.object(self.m,'app_info',return_value={});installed.start();self.addCleanup(installed.stop)
  bundles=patch.object(self.m,'installed_app_path',return_value=None);bundles.start();self.addCleanup(bundles.stop)
  # A tool's other apps and CLIs (Antigravity beside Gemini CLI, Cursor CLI) are listed only where a test installs them.
  extras=patch.object(self.m,'product_installed',return_value=False);extras.start();self.addCleanup(extras.stop)
  # No mirror is reached and every download source answers, unless a test says otherwise: tests stay off the network.
  self.offline={}
  for name,value in (('mirror_fetch',None),('reachable',True)):
   stub=self.offline[name]=patch.object(self.m,name,return_value=value);stub.start();self.addCleanup(stub.stop)
 def row(self,version='4.16336.0'):
  return {'id':'claude','name':'Claude','latest':version,'current':'4.4212.0','available':True,'checkedAt':time.time()}
 def test_numeric_versions_and_unknown_values(self):
  self.assertTrue(self.m.newer_release('4.16336.0','4.4212.0'))
  self.assertFalse(self.m.newer_release('2.1.99','2.1.308'))
  self.assertFalse(self.m.newer_release('garbage 999','2.1.308'))
  self.assertFalse(self.m.newer_release('2.1.318',None))
 def test_notifications_deduplicate_across_cache_refresh_and_restart(self):
  with patch.object(self.m,'send_update_notification',return_value=True) as send:
   self.m.notify_app_updates([self.row()]);self.m.notify_app_updates([self.row()])
   send.assert_called_once()
   self.m.notify_app_updates([self.row('4.16337.0')]);self.assertEqual(send.call_count,2)
  state=json.loads(Path(self.m.STATE_DIR,'updates-notified.json').read_text())
  self.assertEqual(state['claude'],['4.16336.0','4.16337.0'])
  with patch.object(self.m,'send_update_notification') as send:
   self.m.notify_app_updates([self.row()]);send.assert_not_called()
 def test_failed_delivery_is_retryable(self):
  with patch.object(self.m,'send_update_notification',return_value=False) as send:
   self.m.notify_app_updates([self.row()]);self.m.notify_app_updates([self.row()]);self.assertEqual(send.call_count,2)
 def test_stale_or_failed_checks_do_not_notify(self):
  stale=dict(self.row(),checkedAt=time.time()-30000);failed=dict(self.row(),available=False)
  with patch.object(self.m,'send_update_notification') as send:
   self.m.notify_app_updates([stale,failed]);send.assert_not_called()
 def test_network_error_does_not_claim_current_version_is_latest(self):
  with patch.object(self.m,'app_info',return_value={'CFBundleShortVersionString':'4.4212.0'}),patch.object(self.m,'release_body',side_effect=TimeoutError):
   row=self.m.app_update_probe('claude')
  self.assertFalse(row['available']);self.assertIn('失败',row['status'])
 def test_failed_check_says_what_stopped_it(self):
  # 检查失败 alone could not tell a Python without root certificates from a blocked network.
  errors=self.m.urllib.error
  cases=[(errors.URLError(self.m.ssl.SSLCertVerificationError(1,'[SSL: CERTIFICATE_VERIFY_FAILED] certificate verify failed')),'api.anthropic.com 证书验证失败（Python 缺少根证书）'),
         (errors.URLError(self.m.socket.timeout('timed out')),'api.anthropic.com 连接超时（检查网络或代理）'),
         (errors.HTTPError('https://api.anthropic.com/x',403,'Forbidden',{},None),'api.anthropic.com 返回 HTTP 403')]
  for error,reason in cases:
   with patch.object(self.m,'app_info',return_value={'CFBundleShortVersionString':'4.4212.0'}),patch.object(self.m.urllib.request,'urlopen',side_effect=error):
    row=self.m.app_update_probe('claude')
   self.assertEqual((row['status'],row['reason']),('检查失败，稍后重试',reason))
  with patch.object(self.m,'update_rows',return_value=[row]):
   detail=next(l for l in self.m.app_update_lines({}) if 'paneldetail=' in l)
  self.assertIn('返回 HTTP 403',detail)
 def test_plugin_update_notices_say_it_was_the_plugin(self):
  with patch.object(self.m,'install_release'),patch.object(self.m,'notify') as notice,patch.object(self.m.subprocess,'run'),patch.object(self.m,'refresh_swiftbar'):
   self.m.self_update_worker('99.0.0')
  self.assertEqual(notice.call_args.args[0],'插件已更新到 99.0.0');self.assertEqual(self.m.self_update_state()['status'],'done')
  with patch.object(self.m,'install_release',side_effect=self.m.SelfUpdateError('安装包校验失败')),patch.object(self.m,'notify') as notice,patch.object(self.m,'refresh_swiftbar'):
   self.m.self_update_worker('99.0.0')
  self.assertTrue(notice.call_args.args[0].startswith('插件更新失败'))
 def test_plugin_release_row_appears_only_when_newer(self):
  import urllib.error
  def release(body=None,code=None):
   def answer(req,timeout):
    self.assertEqual(req.full_url,self.m.RELEASES_API)
    if code:raise urllib.error.HTTPError(req.full_url,code,'x',{},None)
    response=MagicMock();response.read.return_value=json.dumps(body).encode();response.__enter__.return_value=response;return response
   with patch.object(self.m.urllib.request,'urlopen',side_effect=answer):return self.m.self_update_load()[0]
  self.assertEqual(release(code=404),{'latest':None})
  newer=release({'tag_name':'v99.0.0','html_url':self.m.REPOSITORY+'/releases/tag/v99.0.0','assets':[]})
  self.assertEqual(newer['latest'],'99.0.0')
  self.assertEqual(release({'tag_name':'v99.0.0','draft':True}),{'latest':None})
  self.assertEqual(release({'tag_name':'v99.0.0','html_url':'https://evil.test/x'})['page'],self.m.REPOSITORY+'/releases')
  self.assertEqual(self.m.self_update_lines({'self_update':{'latest':self.m.VERSION}}),[])
  self.assertEqual(self.m.self_update_lines({}),[])
  # Under 工具更新 on the homepage, with the new version and a link to its notes.
  with patch.object(self.m,'update_rows',return_value=[]),patch.object(self.m.terminal_jobs,'latest_log',return_value=None):
   entries=self.m.resource_panel_entries({'self_update':newer})
  labels=[e['label'] for e in entries if not e.get('separator')]
  self.assertEqual(labels[labels.index('工具更新')+1],'Starmeter')
  row=next(e for e in entries if e['label']=='Starmeter')
  self.assertEqual(row['value'],'可更新 99.0.0');self.assertTrue(row['attention'])
  self.assertTrue(any(c.get('href')==newer['page'] for c in row['children']))
  with patch.dict(os.environ,{'OCX_SELF_UPDATES':'0'}):self.assertNotIn('self_update',self.m.external_loaders())
 def listing(self,version,files):
  """The asset list a release page loads, shaped like GitHub's: a label in place of the name, then the digest."""
  folder='/timecatbit/starmeter/releases/download/v%s/'%version
  rows=''.join('<li class="Box-row"><a href="%s%s" rel="nofollow"><span class="text-bold">Label</span></a>'
               '<span class="Truncate-text">sha256:%s</span></li>\n'%(folder,name,digest) for name,digest in files.items())
  return ('<ul>'+rows+'<li class="Box-row"><a href="/timecatbit/starmeter/archive/refs/tags/v%s.zip">Source code</a></li></ul>'%version).encode()
 def test_plugin_release_remains_installable_when_github_api_is_rate_limited(self):
  import urllib.error
  repo=self.m.REPOSITORY
  page=repo+'/releases/tag/v99.0.0'
  name='starmeter-99.0.0.zip';asset=repo+'/releases/download/v99.0.0/'+name
  windows='starmeter-99.0.0-windows-x64.zip'
  digest='a'*64
  blocked=urllib.error.HTTPError(self.m.RELEASES_API,403,'API rate limit exceeded',{},None)
  response=MagicMock();response.__enter__.return_value=response;response.geturl.return_value=page
  listing=self.listing('99.0.0',{name:digest,windows:'b'*64})
  with patch.object(self.m.urllib.request,'urlopen',side_effect=[blocked,response]) as fetch,patch.object(self.m,'download',return_value=listing) as download:
   release,error=self.m.self_update_load()
  self.assertIsNone(error)
  self.assertEqual(fetch.call_args.args[0].full_url,repo+'/releases/latest')
  download.assert_called_once_with(repo+'/releases/expanded_assets/v99.0.0',1024*1024)
  self.assertEqual(release['assets'][0],{'name':name,'url':asset,'digest':'sha256:'+digest})
  self.assertEqual([a['name'] for a in release['assets']],[name,windows])
  self.assertTrue(any('更新到 99.0.0' in row for row in self.m.self_update_lines({'self_update':release})))
  # A release without this copy's file does not turn a failed lookup into an update offer.
  with patch.object(self.m.urllib.request,'urlopen',side_effect=[blocked,response]),patch.object(self.m,'download',return_value=self.listing('99.0.0',{windows:digest})):
   self.assertEqual(self.m.self_update_load(),(None,(403,'检查失败',None)))
  # The Windows app checks for its own archive, so a rate-limited lookup still offers it the release.
  with patch.object(self.m,'DESKTOP',object()),patch.object(self.m.urllib.request,'urlopen',side_effect=[blocked,response]),\
       patch.object(self.m,'download',return_value=listing):
   release,error=self.m.self_update_load()
   self.assertIsNone(error)
   self.assertTrue(any('可更新 99.0.0' in row for row in self.m.self_update_lines({'self_update':release})))

 def redirect(self,mirror,version):
  return 302,mirror+self.m.REPOSITORY+'/releases/tag/v'+version,''
 def mirrors(self,answers):
  """Mirrors answering the latest page and the asset list as given; the rest out of reach."""
  def fetch(url):
   for mirror,latest,listing in answers:
    if url.startswith(mirror):
     if url.endswith('/releases/latest'):return latest
     return (200,None,listing.decode()) if listing else None
   return None
  return fetch
 def test_two_mirrors_agreeing_give_the_release_when_github_cannot_be_reached(self):
  good,forged='a'*64,'b'*64;name='starmeter-99.0.0.zip'
  fast,net,proxy=self.m.GITHUB_MIRRORS
  agreeing=self.mirrors([(fast,self.redirect(fast,'99.0.0'),self.listing('99.0.0',{name:good})),
                         (net,self.redirect(net,'99.0.0'),self.listing('99.0.0',{name:good}))])
  release=self.m.mirrored_release(self.m.mirror_list(),'6.0.14',agreeing)
  self.assertEqual((release['latest'],release['via']),('99.0.0',net))
  self.assertEqual(release['assets'],[{'name':name,'url':self.m.REPOSITORY+'/releases/download/v99.0.0/'+name,'digest':'sha256:'+good}])
  # One mirror alone, or two that differ, give nothing newer; a third agreeing with one settles it.
  alone=self.mirrors([(fast,self.redirect(fast,'99.0.0'),self.listing('99.0.0',{name:good}))])
  self.assertIsNone(self.m.mirrored_release(self.m.mirror_list(),'6.0.14',alone))
  differ=[(fast,self.redirect(fast,'99.0.0'),self.listing('99.0.0',{name:forged})),(net,self.redirect(net,'99.0.0'),self.listing('99.0.0',{name:good}))]
  self.assertIsNone(self.m.mirrored_release(self.m.mirror_list(),'6.0.14',self.mirrors(differ)))
  page=(200,None,'<a href="/timecatbit/starmeter/releases/tag/v99.0.0">')
  settled=self.m.mirrored_release(self.m.mirror_list(),'6.0.14',self.mirrors(differ+[(proxy,page,self.listing('99.0.0',{name:good}))]))
  self.assertIn(good,settled['assets'][0]['digest'])
  # The user's own mirror is taken alone (https only); a version no newer needs no second.
  own=self.m.mirror_list('https://mirror.example , http://plain.example/')
  self.assertEqual(own[0],('https://mirror.example/',True));self.assertTrue(all(m.startswith('https://') for m,_ in own))
  mine=self.mirrors([('https://mirror.example/',self.redirect('https://mirror.example/','99.0.0'),self.listing('99.0.0',{name:good}))])
  self.assertEqual(self.m.mirrored_release(own,'6.0.14',mine)['via'],'https://mirror.example/')
  self.assertEqual(self.m.mirrored_release(self.m.mirror_list(),'6.0.14',self.mirrors([(fast,self.redirect(fast,'6.0.14'),None)])),
                   {'latest':'6.0.14','page':self.m.REPOSITORY+'/releases/tag/v6.0.14','assets':[]})
  # A mirror whose cache is behind doesn't hide a release two others agree on; beside one alone it gives no newer.
  behind=(fast,self.redirect(fast,'6.0.14'),None)
  ahead=[(m,self.redirect(m,'99.0.0'),self.listing('99.0.0',{name:good})) for m in (net,proxy)]
  self.assertEqual(self.m.mirrored_release(self.m.mirror_list(),'6.0.14',self.mirrors([behind]+ahead))['latest'],'99.0.0')
  self.assertEqual(self.m.mirrored_release(self.m.mirror_list(),'6.0.14',self.mirrors([behind]+ahead[:1]))['latest'],'6.0.14')
  # self_update_load turns to them when GitHub can't be reached or fails, not when it says there's no release.
  import urllib.error
  for failure in (urllib.error.URLError('unreachable'),urllib.error.HTTPError(self.m.RELEASES_API,502,'x',{},None)):
   with patch.object(self.m.urllib.request,'urlopen',side_effect=failure),patch.object(self.m,'mirror_fetch',side_effect=agreeing):
    release,error=self.m.self_update_load()
   self.assertIsNone(error);self.assertEqual(release['via'],net)
  with patch.object(self.m.urllib.request,'urlopen',side_effect=urllib.error.URLError('x')):
   self.assertEqual(self.m.self_update_load(),(None,(0,'检查失败',None)))
  with patch.object(self.m.urllib.request,'urlopen',side_effect=urllib.error.HTTPError(self.m.RELEASES_API,404,'x',{},None)),\
       patch.object(self.m,'mirror_fetch',side_effect=AssertionError('asked a mirror')):
   self.assertEqual(self.m.self_update_load(),({'latest':None},None))
 def test_the_file_comes_from_the_first_source_that_has_it_right(self):
  import hashlib
  data=b'the package';digest=hashlib.sha256(data).hexdigest()
  sources=['https://github.com/x','https://bad.example/x','https://good.example/x']
  copies={'https://bad.example/x':b'something else','https://good.example/x':data}
  asked=[]
  def get(url,limit):
   asked.append(url)
   if url not in copies:raise OSError('unreachable')
   return copies[url]
  self.assertEqual(self.m.checked_from(sources,digest,get,probe=lambda url:url!='https://github.com/x'),data)
  self.assertEqual(asked,['https://bad.example/x','https://good.example/x'])
  with self.assertRaisesRegex(self.m.SelfUpdateError,'GitHub 和镜像都连不上'):
   self.m.checked_from(sources,digest,lambda url,limit:(_ for _ in ()).throw(OSError()),probe=lambda url:False)
  with self.assertRaisesRegex(self.m.SelfUpdateError,'安装包校验失败'):self.m.checked_from(sources,digest,lambda url,limit:b'forged')
  # A release read through a mirror is fetched from it first, GitHub last.
  expected=self.m.REPOSITORY+'/releases/download/v99.0.0/starmeter-99.0.0.zip'
  fast,net,proxy=self.m.GITHUB_MIRRORS
  self.assertEqual(self.m.release_sources({'via':net},expected),[net+expected,fast+expected,proxy+expected,expected])
  self.assertEqual(self.m.release_sources({},expected),[expected,fast+expected,net+expected,proxy+expected])
 def test_a_mirror_page_is_read_without_following_its_redirect(self):
  import http.server,threading
  class Answer(http.server.BaseHTTPRequestHandler):
   def do_GET(self):
    if self.path.endswith('/latest'):
     self.send_response(302);self.send_header('Location','https://mirror.test/https://github.com/timecatbit/starmeter/releases/tag/v99.0.0');self.end_headers()
    else:
     body=b'<li>listing</li>';self.send_response(200);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
   def log_message(self,*arguments):pass
  server=http.server.HTTPServer(('127.0.0.1',0),Answer);threading.Thread(target=server.serve_forever,daemon=True).start()
  self.addCleanup(server.server_close);self.addCleanup(server.shutdown)
  base='http://127.0.0.1:%d/'%server.server_port
  self.offline['mirror_fetch'].stop()
  answer=self.m.mirror_fetch(base+'x/releases/latest')
  self.assertEqual(answer[:2],(302,'https://mirror.test/https://github.com/timecatbit/starmeter/releases/tag/v99.0.0'))
  self.assertEqual(self.m.named_version(answer),'99.0.0')
  self.assertEqual(self.m.mirror_fetch(base+'x/releases/expanded_assets/v99.0.0'),(200,None,'<li>listing</li>'))
  self.assertIsNone(self.m.mirror_fetch('http://127.0.0.1:1/unreachable'))

 def test_a_failed_check_waits_ten_minutes_once_not_again_at_each_call(self):
  record={'payload':{'apps':[]}}
  self.assertTrue(self.m.app_update_back_off(record,1000.0));self.assertEqual(record['retryAt'],1600.0)
  self.assertFalse(self.m.app_update_back_off(record,1300.0));self.assertEqual(record['retryAt'],1600.0)
  self.assertTrue(self.m.app_update_back_off(record,1700.0));self.assertEqual(record['retryAt'],2300.0)

 def test_update_rows_go_by_their_light_newer_first_then_up_to_date_then_failed_then_unchecked(self):
  rows=[{'id':'a','name':'A','status':'在 App 内检查更新','manual':True},{'id':'b','name':'B','status':'已是最新','current':'1'},
        {'id':'c','name':'C','status':'有新版本','available':True,'current':'1','latest':'2'},{'id':'d','name':'D','status':'检查失败，稍后重试'},
        {'id':'e','name':'E','status':'已是最新','current':'3'}]
  for row in rows:row['href']='https://example.test/'+row['id']
  with patch.object(self.m,'update_rows',return_value=rows),patch.object(self.m.terminal_jobs,'latest_log',return_value=None):
   lines=self.m.app_update_lines({})
  names=[line.split(' · ')[0] for line in lines[1:] if line.startswith('--') and ' · ' in line]
  self.assertEqual([name.lstrip('-') for name in names][:5],['C','B','E','D','A'])

 def test_each_copy_updates_from_a_file_the_release_publishes(self):
  import importlib.util,sys
  scripts=str(SOURCE.parent/'scripts')
  sys.path.insert(0,scripts);self.addCleanup(sys.path.remove,scripts)
  spec=importlib.util.spec_from_file_location('package_script',SOURCE.parent/'scripts/package.py')
  package=importlib.util.module_from_spec(spec);spec.loader.exec_module(package)
  published={name for name,_ in package.downloads('99.0.0')}
  self.assertEqual(len(published),9)
  self.assertIn(self.m.update_asset_name('99.0.0'),published)
  with patch.object(self.m,'DESKTOP',object()):self.assertIn(self.m.update_asset_name('99.0.0'),published)
  # The Linux tray looks for its own package, not the Windows archive.
  with patch.object(self.m,'DESKTOP',object()),patch.object(self.m.platform,'system',return_value='Linux'):
   self.assertEqual(self.m.update_asset_name('99.0.0'),'starmeter-99.0.0-linux-x64.AppImage')
   self.assertIn(self.m.update_asset_name('99.0.0'),published)
  for machine in ('arm64','x86_64'):
   with patch.object(self.m,'MAC_APP',True),patch.object(self.m.platform,'machine',return_value=machine):
    self.assertIn(self.m.update_asset_name('99.0.0'),published)
  # The name installed SwiftBar plugins fetch (starmeter-… from 6.0.2; 6.0.1 and earlier looked for token-monitor-…).
  self.assertIn('starmeter-99.0.0.zip',published)

 def test_mac_app_only_offers_its_own_architecture(self):
  release={'latest':'99.0.0','assets':[{'name':'starmeter-99.0.0.zip'},
                                      {'name':'starmeter-99.0.0-macos-intel.dmg'}]}
  self.assertEqual(self.m.update_asset_name('99.0.0'),'starmeter-99.0.0.zip')
  with patch.object(self.m,'MAC_APP',True),patch.object(self.m.platform,'machine',return_value='arm64'):
   self.assertEqual(self.m.self_update_lines({'self_update':release}),[])
   release['assets'].append({'name':'starmeter-99.0.0-macos-apple-silicon.dmg'})
   lines='\n'.join(self.m.self_update_lines({'self_update':release}))
   self.assertIn('可更新 99.0.0',lines)
   self.assertIn('releases/tag/v99.0.0',lines)
   self.assertNotIn('update-self',lines)
  with patch.object(self.m,'MAC_APP',True),patch.object(self.m.platform,'machine',return_value='x86_64'):
   self.assertEqual(self.m.update_asset_name('99.0.0'),'starmeter-99.0.0-macos-intel.dmg')

 def test_self_update_verifies_the_archive_and_runs_its_installer(self):
  import hashlib,io,zipfile
  root=Path(self.tmp.name);marker=root/'installed.json'
  def bundle(entries):
   data=io.BytesIO()
   with zipfile.ZipFile(data,'w') as z:
    for name,text in entries.items():z.writestr(name,text)
   return data.getvalue()
  good=bundle({'starmeter-99.0.0/install.py':'import json,sys;open(%r,"w").write(json.dumps(sys.argv[1:]))'%str(marker)})
  def release(archive,digest=None):
   url=self.m.REPOSITORY+'/releases/download/v99.0.0/starmeter-99.0.0.zip'
   payload={'latest':'99.0.0','page':self.m.REPOSITORY+'/releases','assets':[{'name':'starmeter-99.0.0.zip','url':url,'digest':'sha256:'+(digest or hashlib.sha256(archive).hexdigest())}]}
   self.m.write_json_cache(self.m.cache_path('self_update'),{'schema':1,'payload':payload,'fetchedAt':time.time()})
  log=str(root/'logs'/'self-update.log')
  release(good)
  with patch.object(self.m,'download',return_value=good):self.m.install_release('99.0.0',log)
  self.assertEqual(json.loads(marker.read_text()),['--plugin-dir',self.m.PLUGIN_DIR,'--no-refresh'])
  self.assertEqual(oct(Path(log).stat().st_mode&0o777),'0o600')
  # A wrong checksum or a path outside the release folder stops before anything runs.
  marker.unlink();release(good,digest='0'*64)
  with patch.object(self.m,'download',return_value=good),self.assertRaises(self.m.SelfUpdateError):self.m.install_release('99.0.0',log)
  evil=bundle({'starmeter-99.0.0/../../escape.py':'x'});release(evil)
  with patch.object(self.m,'download',return_value=evil),self.assertRaises(self.m.SelfUpdateError):self.m.install_release('99.0.0',log)
  self.assertFalse(marker.exists())
  # Only this repository's release URL is accepted.
  payload=self.m.read_cache('self_update')['payload'];payload['assets'][0]['url']='https://evil.test/starmeter-99.0.0.zip'
  self.m.write_json_cache(self.m.cache_path('self_update'),{'schema':1,'payload':payload,'fetchedAt':time.time()})
  with patch.object(self.m,'download',side_effect=AssertionError('downloaded')),self.assertRaises(self.m.SelfUpdateError):self.m.install_release('99.0.0',log)
  # Without GitHub's digest nothing is downloaded: releases carry no separate checksum files.
  payload['assets'][0].update(url=self.m.REPOSITORY+'/releases/download/v99.0.0/starmeter-99.0.0.zip',digest='')
  self.m.write_json_cache(self.m.cache_path('self_update'),{'schema':1,'payload':payload,'fetchedAt':time.time()})
  with patch.object(self.m,'download',side_effect=AssertionError('downloaded')),self.assertRaisesRegex(self.m.SelfUpdateError,'没有校验值'):self.m.install_release('99.0.0',log)
 def test_self_update_row_offers_update_then_shows_progress(self):
  data={'self_update':{'latest':'99.0.0','page':self.m.REPOSITORY+'/releases','assets':[]}}
  self.assertTrue(any('param3=update-self' in line for line in self.m.self_update_lines(data)))
  self.m.set_self_update_state(status='running',version='99.0.0')
  rows='\n'.join(self.m.self_update_lines(data))
  self.assertIn('正在更新',rows);self.assertNotIn('update-self',rows)
  self.m.set_self_update_state(status='failed',version='99.0.0',reason='安装包校验失败')
  rows='\n'.join(self.m.self_update_lines(data))
  self.assertIn('安装包校验失败',rows);self.assertIn('重试更新到 99.0.0',rows)
  # Opening the row doesn't ask GitHub again; the release check stays at once every 6 hours.
  self.m.write_json_cache(self.m.cache_path('self_update'),{'schema':1,'fetchedAt':time.time(),'payload':data['self_update']})
  row=next(e for e in self.m.resource_panel_entries(self.m.cached_data()) if e['label']==self.m.PRODUCT)
  self.assertNotIn('onOpenAction',row)
  with self.assertRaises(ValueError):self.m.refresh_resource('self-update')
 def test_update_menu_prioritizes_new_versions_and_preserves_actions(self):
  rows=[{'id':key,'name':name,'status':status,'available':available,'href':'https://example.test/'+key,'current':'1.0','latest':'2.0'} for key,name,status,available in [
   ('opencodex','OpenCodex','自动更新已开启 · 已是最新',False),
   ('claude','Claude','有新版本',True),('claude-code','Claude Code','有新版本',True),
   ('codex-cli','Codex','检查失败，稍后重试',False),('chatgpt','ChatGPT','等待检查',False)]]
  rows[2]['command']='brew upgrade --cask claude-code'
  with patch.object(self.m,'update_rows',return_value=rows),patch.object(self.m.terminal_jobs,'latest_log',return_value=None):
   entries=self.m.panel_entries(self.m.app_update_lines({}))[0]['children'][:5]
  self.assertEqual([e['label'].split(' · ')[0] for e in entries],['Claude','Claude Code','OpenCodex','Codex','ChatGPT'])
  self.assertEqual([e['statusLight'] for e in entries],['new','new','ok','issue','unknown'])
  self.assertEqual(entries[1]['action'],['update-cli','claude-code'])
  self.assertIn('点击运行 brew upgrade --cask claude-code',entries[1]['detail'])
  self.assertEqual(entries[0]['href'],'https://example.test/claude')
 def test_claude_checks_the_desktop_app_and_official_desktop_feed(self):
  with patch.object(self.m,'app_info',return_value={'CFBundleShortVersionString':'4.4212.0'}) as info,patch.object(self.m,'release_body',return_value=b'{"currentRelease":"4.16336.0"}') as release,patch.object(self.m.subprocess,'run') as command:
   row=self.m.app_update_probe('claude')
  info.assert_called_once_with('Claude');command.assert_not_called()
  self.assertIn('/api/desktop/darwin/universal/squirrel/update?',release.call_args.args[0])
  self.assertEqual(row['name'],'Claude');self.assertEqual(row['current'],'4.4212.0');self.assertTrue(row['available'])
 def test_missing_claude_app_does_not_fall_back_to_cli(self):
  with patch.object(self.m,'release_body') as release,patch.object(self.m.subprocess,'run') as command:
   row=self.m.app_update_probe('claude')
  self.assertEqual(row['status'],'未安装');release.assert_not_called();command.assert_not_called()
 def test_claude_selection_includes_code_update_and_notification(self):
  self.select('claude')
  cli=dict(self.row(),id='claude-code',name='Claude Code')
  rows=self.m.update_rows({'app_updates':{'apps':[cli]}})
  self.assertEqual([r['id'] for r in rows],['claude','claude-code']);self.assertEqual(self.m.update_attention_count(rows),1)
  with patch.object(self.m,'send_update_notification',return_value=True) as send:
   self.m.notify_app_updates([cli]);send.assert_called_once()
 def test_sparkle_uses_build_and_filters_other_architecture(self):
  xml=b'''<rss xmlns:s="http://www.andymatuschak.org/xml-namespaces/sparkle"><channel><item><s:shortVersionString>26.1029.61003</s:shortVersionString><s:version>24614</s:version><s:hardwareRequirements>arm64</s:hardwareRequirements></item><item><s:shortVersionString>99.0.0</s:shortVersionString><s:version>99999</s:version><s:hardwareRequirements>x64</s:hardwareRequirements></item></channel></rss>'''
  with patch.object(self.m.platform,'machine',return_value='arm64'):
   self.assertEqual(self.m.sparkle_release(xml),('26.1029.61003','24614'))
 def test_chatgpt_native_rollout_overrides_anonymous_feed_and_network_failure(self):
  info={'CFBundleIdentifier':'com.openai.codex','CFBundleShortVersionString':'26.1029.61003','CFBundleVersion':'24614'}
  native={'latest':'26.1103.31037','build':'25241','releaseSource':'client-download','observedAt':time.time()}
  with patch.object(self.m,'app_info',return_value=info),patch.object(self.m.desktop_updates,'chatgpt_release',return_value=native):
   with patch.object(self.m,'release_body',return_value=b'<rss/>'),patch.object(self.m,'sparkle_release',return_value=('26.1029.61003','24614')):
    row=self.m.app_update_probe('chatgpt')
   self.assertTrue(row['available']);self.assertEqual(row['latest'],'26.1103.31037');self.assertEqual(row['channel'],'已下载')
   self.assertEqual(self.m.update_indicator(row),'new')
   with patch.object(self.m,'release_body',side_effect=TimeoutError):row=self.m.app_update_probe('chatgpt')
   self.assertTrue(row['available']);self.assertEqual(row['status'],'有新版本')
 def test_chatgpt_anonymous_or_empty_feed_never_claims_account_is_current(self):
  info={'CFBundleIdentifier':'com.openai.codex','CFBundleShortVersionString':'26.1029.61003','CFBundleVersion':'24614'}
  with patch.object(self.m,'app_info',return_value=info),patch.object(self.m.desktop_updates,'chatgpt_release',return_value=None),patch.object(self.m,'release_body',return_value=b'<rss/>'),patch.object(self.m,'sparkle_release',return_value=('26.1029.61003','24614')) as release:
   row=self.m.app_update_probe('chatgpt');self.assertEqual(row['status'],'公开渠道暂无更新');self.assertEqual(self.m.update_indicator(row),'unknown')
   release.return_value=(None,None);row=self.m.app_update_probe('chatgpt')
   self.assertIn('失败',row['status']);self.assertFalse(row['available'])
 def test_native_download_change_invalidates_update_cache_and_install_clears_alert(self):
  self.select('codex')
  info={'CFBundleIdentifier':'com.openai.codex','CFBundleShortVersionString':'26.1029.61003','CFBundleVersion':'24614'}
  native={'latest':'26.1103.31037','build':'25241','releaseSource':'client-download'}
  with patch.object(self.m,'app_info',return_value=info),patch.object(self.m.desktop_updates,'chatgpt_release',return_value=None) as local:
   before=self.m.connection_fingerprint('app_updates');local.return_value=native
   self.assertNotEqual(before,self.m.connection_fingerprint('app_updates'))
   row=self.m.apply_chatgpt_release({'id':'chatgpt'},info);self.assertTrue(row['available'])
   info.update(CFBundleShortVersionString='26.1103.31037',CFBundleVersion='25241')
   self.assertFalse(self.m.apply_chatgpt_release(row,info)['available'])
 def test_installed_desktop_update_immediately_clears_old_alert(self):
  self.m.preferences.update(self.m.STATE_DIR,'module','codex','true')
  row=dict(self.row(),id='chatgpt',latest='4.16336.0')
  with patch.object(self.m,'app_info',return_value={'CFBundleShortVersionString':'4.16336.0'}):
   rows=self.m.update_rows({'app_updates':{'apps':[row]}})
  self.assertFalse(next(r for r in rows if r['id']=='chatgpt')['available'])
 def test_home_balances_are_visible_and_not_nested_under_management(self):
  data={'healthz':{'version':'2.97.0'},'provider_quotas':{'reports':[{'provider':'deepseek','quota':{'customWindows':[{'label':'¥229.61'}]}}]},'accounts':{'accounts':[{'id':'a','logLabel':'main'}]},'quota':{'quotas':{'a':{'weeklyPercent':76}}}}
  with patch.object(self.m,'deepseek_logo_image',return_value=None),patch.object(self.m,'chatgpt_source',return_value='proxy'),patch.object(self.m,'default_ready',side_effect=lambda key:key=='deepseek'):
   entries=self.m.panel_entries(self.m.management_lines(data))
  by_name={e['label']:e for e in entries}
  self.assertIn('229.61',by_name['DeepSeek API']['value'])
  # Subscriptions report the share used, like Claude, with the same 70%/90% colours.
  chatgpt=by_name['ChatGPT 订阅'];self.assertEqual((chatgpt['value'],chatgpt['usageLevel']),('已用 76%','warning'))
  self.assertIn(('main','已用 76%'),[(e['label'],e.get('value')) for e in chatgpt['children']])
  management=next(e for e in entries if e['label']=='OpenCodex 管理')
  self.assertFalse(any('DeepSeek' in e['label'] for e in management['children']))
 def select(self,*harnesses,updates=True):
  settings=self.m.preferences.read(self.m.STATE_DIR)
  settings['visible']=list(harnesses)+(['updates'] if updates else [])
  self.m.preferences.write(self.m.STATE_DIR,settings)
 def test_default_updates_follow_only_selected_harnesses(self):
  self.assertEqual(self.m.selected_update_ids(),['opencodex','chatgpt','codex-cli','claude','claude-code'])
  self.assertEqual([r['id'] for r in self.m.update_rows({})],['opencodex','chatgpt','codex-cli','claude','claude-code'])
 def test_configured_opencodex_also_checks_chatgpt_without_changing_home_selection(self):
  self.select('claude')
  before=self.m.preferences.read(self.m.STATE_DIR)
  with patch.object(self.m,'ocx_configured',return_value=True),patch.object(self.m,'app_info',return_value={}),patch.object(self.m,'app_update_probe',side_effect=lambda key:dict(self.row(),id=key,href=self.m.UPDATE_LINKS[key],status='有新版本')) as probe:
   self.assertEqual(self.m.selected_update_ids(),['chatgpt','codex-cli','claude','claude-code'])
   self.m.app_updates_cached()
   self.assertCountEqual([c.args[0] for c in probe.call_args_list],['claude','claude-code','chatgpt','codex-cli'])
   self.assertNotIn('badge',self.m.external_loaders())
   self.assertIn('选择工具…','\n'.join(self.m.app_update_lines({})))
  self.assertEqual(self.m.preferences.read(self.m.STATE_DIR),before)
 def test_configured_opencodex_adds_chatgpt_once_even_with_no_selected_harness(self):
  with patch.object(self.m,'ocx_configured',return_value=True):
   for selected in ((),('codex',)):
    self.select(*selected)
    self.assertEqual(self.m.selected_update_ids(),['chatgpt','codex-cli'])
    with patch.object(self.m,'app_info',return_value={}):
     self.assertEqual([r['id'] for r in self.m.update_rows({})],['chatgpt','codex-cli'])
 def test_opencodex_presence_changes_invalidate_cache_and_remove_unselected_chatgpt_alerts(self):
  self.select('claude')
  with patch.object(self.m,'ocx_configured',return_value=False) as configured,patch.object(self.m,'app_update_probe',side_effect=lambda key:dict(self.row(),id=key,href='https://example.test',status='有新版本')) as probe:
   self.m.app_updates_cached()
   configured.return_value=True
   self.assertEqual(self.m.read_cache('app_updates'),{})
   self.m.app_updates_cached()
   self.assertCountEqual([c.args[0] for c in probe.call_args_list],['claude','claude-code','claude','claude-code','chatgpt','codex-cli'])
   payload=self.m.read_cache('app_updates')['payload']
   linked=[r for r in payload['apps'] if r['id']=='chatgpt']
   with patch.object(self.m,'send_update_notification',return_value=True) as send:
    self.m.notify_app_updates(linked);send.assert_called_once()
   configured.return_value=False
   self.assertEqual(self.m.read_cache('app_updates'),{})
   rows=self.m.update_rows({'app_updates':payload})
   self.assertEqual([r['id'] for r in rows],['claude','claude-code'])
   self.assertEqual(self.m.update_attention_count(rows),2)
   with patch.object(self.m,'send_update_notification') as send:
    self.m.notify_app_updates([dict(linked[0],latest='99.0.0')]);send.assert_not_called()
 def test_subscription_and_api_choices_do_not_select_app_updates(self):
  self.select('chatgpt','claude-subscription','deepseek','gemini-api')
  with patch.object(self.m,'app_update_probe') as probe,patch.object(self.m,'update_status_cached') as proxy:
   self.assertEqual(self.m.selected_update_ids(),[])
   self.assertEqual(self.m.update_rows({}),[])
   self.m.app_updates_cached();loaders=self.m.external_loaders()
  probe.assert_not_called();proxy.assert_not_called()
  self.assertNotIn('app_updates',loaders);self.assertNotIn('badge',loaders)
  self.assertIn('尚未勾选工具','\n'.join(self.m.app_update_lines({})))
 def test_selected_harness_is_the_only_network_probe(self):
  self.select('claude')
  with patch.object(self.m,'app_update_probe',return_value=self.row()) as probe:
   self.m.app_updates_cached()
  self.assertCountEqual([c.args[0] for c in probe.call_args_list],['claude','claude-code'])
 def test_switching_harness_invalidates_recent_update_cache(self):
  self.select('claude')
  def row(key):return dict(self.row(),id=key)
  with patch.object(self.m,'app_update_probe',side_effect=row) as probe:
   self.m.app_updates_cached();self.select('codex');self.m.app_updates_cached()
  self.assertCountEqual([c.args[0] for c in probe.call_args_list],['claude','claude-code','chatgpt','codex-cli'])
 def test_deselected_cached_update_is_hidden_from_list_badge_and_notification(self):
  self.select('codex')
  with patch.object(self.m,'app_info',return_value={}):rows=self.m.update_rows({'app_updates':{'apps':[self.row()]}})
  self.assertEqual([r['id'] for r in rows],['chatgpt','codex-cli'])
  self.assertEqual(self.m.update_attention_count(rows),0)
  with patch.object(self.m,'send_update_notification') as send:self.m.notify_app_updates([self.row()])
  send.assert_not_called()
 def test_turning_off_update_module_stops_checks_and_notifications(self):
  self.select('claude',updates=False)
  with patch.object(self.m,'ocx_configured',return_value=True),patch.object(self.m,'app_update_probe') as probe,patch.object(self.m,'send_update_notification') as send:
   self.m.app_updates_cached();self.m.notify_app_updates([self.row(),dict(self.row(),id='chatgpt')])
   self.assertNotIn('app_updates',self.m.external_loaders())
  probe.assert_not_called();send.assert_not_called()
 def test_every_selected_tool_is_checked_and_only_those_without_a_published_version_stay_manual(self):
  # The user asked for each tool to be checked, product by product, not a column of 尚未接入自动版本检查.
  self.select('opencode','gemini','magpie','cursor','deepseek-harness','qoder','workbuddy')
  with patch.object(self.m,'app_update_probe',side_effect=lambda key:dict(self.row(),id=key,status='已是最新',available=False)) as probe:
   self.m.app_updates_cached()
  self.assertCountEqual([c.args[0] for c in probe.call_args_list],['magpie','cursor','deepseek-harness','gemini','opencode'])
  versions={'WorkBuddy':{'CFBundleShortVersionString':'5.9.3'}}
  with patch.object(self.m,'app_info',side_effect=lambda name:versions.get(name,{})):
   rows={r['id']:r for r in self.m.update_rows({})}
   text='\n'.join(self.m.app_update_lines({}))
  self.assertEqual([k for k,r in rows.items() if r.get('manual')],['qoder','workbuddy'])
  self.assertNotIn('尚未接入',text)
  self.assertIn('5.9.3 · 腾讯没有公开 WorkBuddy 的版本接口',text)
  self.assertIn('Qoder CN 没有公开 App 的版本接口',text)
  self.assertNotIn('已是最新','\n'.join(l for l in text.split('\n') if 'WorkBuddy' in l or 'Qoder' in l))
 def test_a_tools_apps_and_clis_are_rows_of_their_own_where_installed(self):
  # Gemini: Gemini CLI always, Antigravity's app, IDE and agy CLI and the Gemini app only where this Mac has them.
  self.select('gemini','cursor','opencode','qoder')
  found={'antigravity','antigravity-cli','cursor-cli','opencode-desktop','qoder-cli'}
  with patch.object(self.m,'product_installed',side_effect=lambda product:product['id'] in found):
   self.assertEqual(self.m.selected_update_ids(),['cursor','cursor-cli','gemini','antigravity','antigravity-cli','opencode-desktop','opencode','qoder','qoder-cli'])
   names=[r['name'] for r in self.m.update_rows({})]
  self.assertEqual(names,['Cursor','Cursor CLI','Gemini CLI','Antigravity','Antigravity CLI','OpenCode Desktop','OpenCode','Qoder','Qoder CLI'])
 def test_the_gemini_app_is_a_row_of_its_own_that_googles_updater_keeps_current(self):
  # The user's "gemini" beside Gemini CLI and Antigravity: Google's desktop app, kept current by Google's own updater.
  self.select('gemini')
  with patch.object(self.m,'product_installed',side_effect=lambda product:product['id']=='gemini-app'),\
       patch.object(self.m,'app_info',side_effect=lambda name:{'CFBundleShortVersionString':'1.147.6.1557'} if name=='Gemini' else {}):
   self.assertEqual(self.m.selected_update_ids(),['gemini-app','gemini'])
   self.assertNotIn('gemini-app',self.m.live_app_update_ids())
   text='\n'.join(self.m.app_update_lines({}))
  self.assertIn('Gemini App · ',text);self.assertIn('1.147.6.1557 · 由 Google 更新程序自动更新',text)
 def test_each_cli_is_updated_by_the_channel_it_came_from(self):
  root=Path(self.tmp.name)
  def write(path,text=''):
   path.parent.mkdir(parents=True,exist_ok=True);path.write_text(text)
  claude=root/'Caskroom/claude-code/2.1.311/claude';write(claude)
  write(root/'Caskroom/claude-code/.metadata/INSTALL_RECEIPT.json',json.dumps({'source':{'tap':'homebrew/cask'}}));write(root/'bin/brew')
  gemini=root/'lib/node_modules/@google/gemini-cli/dist/index.js';write(gemini)
  write(root/'lib/node_modules/@google/gemini-cli/package.json',json.dumps({'name':'@google/gemini-cli'}));write(root/'bin/npm')
  binaries={'claude':str(claude),'gemini':str(gemini)}
  self.select('claude','gemini')
  bodies={'https://formulae.brew.sh/api/cask/claude-code.json':b'{"version":"2.1.323"}','https://registry.npmjs.org/@google/gemini-cli/latest':b'{"version":"0.70.0"}'}
  outputs={str(claude):'2.1.311 (Claude Code)',str(gemini):'0.69.1'}
  with patch.object(self.m.shutil,'which',side_effect=binaries.get),patch.object(self.m,'release_body',side_effect=bodies.get),\
       patch.object(self.m.subprocess,'run',side_effect=lambda args,**kw:types.SimpleNamespace(stdout=outputs[args[0]])):
   rows={key:self.m.app_update_probe(key) for key in ('claude-code','gemini')}
  self.assertEqual((rows['claude-code']['channel'],rows['claude-code']['command']),('Homebrew Cask','brew upgrade --cask claude-code'))
  self.assertEqual((rows['gemini']['channel'],rows['gemini']['command']),('npm','npm install -g @google/gemini-cli@latest'))
  self.assertTrue(all(r['available'] for r in rows.values()))
  brew=str((root/'bin/brew').resolve());npm=str((root/'bin/npm').resolve())
  with patch.object(self.m.shutil,'which',side_effect=binaries.get),patch.object(self.m,'terminal') as terminal:
   self.m.run_action('update-cli',['claude-code']);self.m.run_action('update-cli',['gemini'])
  scripts=[c.args[0] for c in terminal.call_args_list]
  self.assertIn(brew+' upgrade --cask claude-code 2>&1',scripts[0]);self.assertIn(npm+' install -g @google/gemini-cli@latest 2>&1',scripts[1])
  self.assertIn(str(gemini)+' --version',scripts[1])
  # The new rows are told about like the others.
  with patch.object(self.m,'send_update_notification',return_value=True) as send:
   self.m.notify_app_updates([dict(rows['gemini'],checkedAt=time.time())])
  send.assert_called_once()
 def test_apps_are_read_against_their_vendors_feeds(self):
  info={'Cursor':{'CFBundleShortVersionString':'3.30.14'},'Antigravity IDE':{'CFBundleShortVersionString':'2.7.0'}}
  feed=json.dumps({'version':'8dc5315a','productVersion':'1.107.0','url':'https://edgedl.me.gvt1.com/x/antigravity/stable/2.8.0-6733/Antigravity.zip'}).encode()
  with patch.object(self.m,'app_info',side_effect=lambda name:info.get(name,{})),patch.object(self.m,'release_body',side_effect=lambda url:b'{"version":"3.31.15"}' if 'cursor.com' in url else feed),\
       patch.object(self.m,'installed_app_path',side_effect=lambda name:'/Applications/%s.app'%name if name in info else None),\
       patch.object(self.m.update_channels,'app_commit',return_value='8dc5315a') as commit:
   cursor=self.m.app_update_probe('cursor');ide=self.m.app_update_probe('antigravity-ide')
   self.assertEqual((cursor['latest'],cursor['available']),('3.31.15',True))
   # The IDE's feed tells releases apart by commit: built from the newest one, it is up to date whatever the numbers.
   self.assertEqual((ide['latest'],ide['available'],ide['status']),('2.8.0',False,'已是最新'))
   commit.return_value='older'
   self.assertTrue(self.m.app_update_probe('antigravity-ide')['available'])
 def test_installed_desktop_apps_open_the_app_instead_of_a_download_page(self):
  self.select('claude','workbuddy','qoder','gemini')
  apps={'Claude':'/Applications/Claude.app','WorkBuddy':'/Users/me/Applications/Work Buddy.app','QoderCN':'/Applications/QoderCN.app'}
  with patch.object(self.m,'installed_app_path',side_effect=apps.get):
   rows={r['id']:r for r in self.m.update_rows({'app_updates':{'apps':[self.row()]}})}
   self.assertEqual(rows['claude']['href'],'file:///Applications/Claude.app')
   self.assertEqual(rows['workbuddy']['href'],'file:///Users/me/Applications/Work%20Buddy.app')
   self.assertEqual(rows['workbuddy']['status'],'在 App 内检查更新')
   self.assertEqual((rows['qoder']['name'],rows['qoder']['href']),('Qoder','file:///Applications/QoderCN.app'))
   self.assertTrue(rows['claude-code']['href'].startswith('https://'))
   # Gemini CLI has a version check of its own now; WorkBuddy and Qoder say why they have none.
   self.assertEqual(rows['gemini']['status'],'等待检查')
   self.assertEqual(rows['qoder']['note'],'Qoder CN 没有公开 App 的版本接口')
   entries=self.m.panel_entries(self.m.app_update_lines({'app_updates':{'apps':[dict(self.row(),status='有新版本')]}}))[0]['children']
  claude=next(e for e in entries if e['label'].startswith('Claude ·'))
  self.assertEqual(claude['href'],'file:///Applications/Claude.app');self.assertIn('点击打开 App 更新',claude['detail'])
  with patch.object(self.m,'installed_app_path',return_value=None):
   rows={r['id']:r for r in self.m.update_rows({})}
  self.assertEqual(rows['claude']['href'],'https://claude.ai/download');self.assertEqual(rows['workbuddy']['status'],'在官网查看更新')
  self.assertEqual(rows['qoder']['href'],'https://qoder.cn/download')
 def test_selection_changed_in_flight_cannot_publish_a_fresh_wrong_scope(self):
  self.select('claude')
  def change_while_loading(key):
   self.select('codex');return self.row()
  with patch.object(self.m,'app_update_probe',side_effect=change_while_loading):self.m.app_updates_cached()
  self.assertEqual(self.m.read_cache('app_updates'),{})
  with patch.object(self.m,'app_update_probe',return_value=dict(self.row(),id='chatgpt')) as probe:self.m.app_updates_cached()
  self.assertCountEqual([c.args[0] for c in probe.call_args_list],['chatgpt','codex-cli'])
 def test_cli_release_sources_and_missing_install(self):
  cases=[('claude-code','2.1.308 (Claude Code)','2.1.310','claude.ai/claude-code-releases/latest'),('codex-cli','codex-cli 0.164.0','0.166.0','registry.npmjs.org/@openai/codex/latest')]
  for key,current,latest,url in cases:
   body=latest.encode() if key=='claude-code' else json.dumps({'version':latest}).encode()
   with patch.object(self.m.shutil,'which',return_value='/test/bin/tool'),patch.object(self.m.subprocess,'run',return_value=types.SimpleNamespace(stdout=current)),patch.object(self.m,'release_body',return_value=body) as release,patch.object(self.m.update_channels,'cli_channel',side_effect=lambda key,binary:{'channel':'Claude 官方','channelId':'text','releaseURL':'https://downloads.claude.ai/claude-code-releases/latest'} if key=='claude-code' else {'channel':'npm','channelId':'npm','releaseURL':'https://registry.npmjs.org/@openai/codex/latest'}):
    row=self.m.app_update_probe(key)
   self.assertTrue(row['available']);self.assertEqual(row['latest'],latest);self.assertIn(url,release.call_args.args[0])
  with patch.object(self.m.shutil,'which',return_value=None),patch.object(self.m,'release_body') as release:
   self.assertEqual(self.m.app_update_probe('codex-cli')['status'],'未安装');release.assert_not_called()
 def test_brew_current_version_never_uses_npm_ahead_of_its_channel(self):
  channel={'channel':'Homebrew Cask','channelId':'brew-cask','releaseURL':'https://formulae.brew.sh/api/cask/codex.json'}
  with patch.object(self.m.shutil,'which',return_value='/test/bin/codex'),patch.object(self.m.subprocess,'run',return_value=types.SimpleNamespace(stdout='codex-cli 0.166.0')),patch.object(self.m.update_channels,'cli_channel',return_value=channel),patch.object(self.m,'release_body',return_value=b'{"version":"0.166.0"}') as release:
   row=self.m.app_update_probe('codex-cli')
  release.assert_called_once_with(channel['releaseURL'])
  self.assertEqual(row['channel'],'Homebrew Cask');self.assertFalse(row['available']);self.assertEqual(row['latest'],'0.166.0')
 def test_cli_update_runs_the_requested_command_with_safe_quoting(self):
  self.select('claude','codex')
  for key,command in [('claude-code','claude'),('codex-cli','codex')]:
   binary='/Applications/Tools With Spaces/'+command
   with patch.object(self.m.shutil,'which',return_value=binary),patch.object(self.m.subprocess,'run',return_value=types.SimpleNamespace(stdout='  update Update the CLI')),patch.object(self.m,'terminal') as terminal:
    self.m.run_action('update-cli',[key])
   self.assertIn("'"+binary+"' update 2>&1",terminal.call_args.args[0])
   self.assertIn('swiftbar://refreshplugin?',terminal.call_args.args[0])
   self.assertIn('token_update_status=${PIPESTATUS[0]}',terminal.call_args.args[0])
   self.assertIn('更新成功',terminal.call_args.args[0]);self.assertIn('更新失败',terminal.call_args.args[0])
 def test_cli_update_refuses_unselected_or_unsupported_command(self):
  self.select('claude')
  with patch.object(self.m,'terminal') as terminal:
   with self.assertRaises(ValueError):self.m.run_action('update-cli',['codex-cli'])
   with patch.object(self.m.shutil,'which',return_value='/test/claude'),patch.object(self.m.subprocess,'run',return_value=types.SimpleNamespace(stdout='  chat Start chat')),patch.object(self.m,'applescript'):
    self.m.run_action('update-cli',['claude-code'])
   terminal.assert_not_called()
 def test_cli_install_change_invalidates_update_cache(self):
  self.select('codex')
  binary=Path(self.tmp.name,'codex');binary.write_text('old')
  with patch.object(self.m.shutil,'which',return_value=str(binary)):
   before=self.m.connection_fingerprint('app_updates');binary.write_text('new installed release')
   self.assertNotEqual(before,self.m.connection_fingerprint('app_updates'))
if __name__=='__main__':unittest.main()
