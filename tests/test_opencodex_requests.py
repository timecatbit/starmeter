import http.server,os,tempfile,threading,types,unittest
from pathlib import Path
from unittest.mock import patch
SOURCE=Path(__file__).resolve().parents[1]/'starmeter.30s.sh'
CODE=(SOURCE.parent/'lib/swiftbar_plugin.py').read_text()


class OpenCodexRequestTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        env=patch.dict(os.environ,{'OCX_CACHE_DIR':self.tmp.name,'SWIFTBAR_PLUGIN_DATA_PATH':self.tmp.name,'OCX_PLUGIN_PATH':str(SOURCE.resolve())})
        env.start();self.addCleanup(env.stop)
        self.m=types.ModuleType('opencodex_requests_test');exec(compile(CODE,str(SOURCE.parent/'lib/swiftbar_plugin.py'),'exec'),self.m.__dict__)

    def test_an_account_in_debt_shows_its_negative_balance(self):
        report=self.m.deepseek_balance_report({'balance_infos':[{'currency':'CNY','total_balance':'-3.20','granted_balance':'0.00','topped_up_balance':'-3.20'}]})
        self.assertEqual(self.m.report_balance_value(report['reports'][0]),-3.2);self.assertIn('¥-3.20',report['reports'][0]['quota']['customWindows'][0]['label'])
        report=self.m.deepseek_balance_report({'balance_infos':[{'currency':'USD','total_balance':'12.50'}]})
        self.assertEqual(self.m.report_balance_value(report['reports'][0]),12.5)

    def test_a_redirect_from_opencodex_is_not_followed_with_the_admin_token(self):
        asked=[]
        class Elsewhere(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                asked.append(self.headers.get('x-opencodex-api-key'));self.send_response(200);self.end_headers()
            def log_message(self,*arguments):pass
        other=http.server.HTTPServer(('127.0.0.1',0),Elsewhere)
        class Redirect(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(302);self.send_header('Location','http://127.0.0.1:%d/steal'%other.server_port);self.end_headers()
            def log_message(self,*arguments):pass
        server=http.server.HTTPServer(('127.0.0.1',0),Redirect)
        for s in (server,other):
            threading.Thread(target=s.serve_forever,daemon=True).start();self.addCleanup(s.server_close);self.addCleanup(s.shutdown)
        with patch.object(self.m,'BASE','http://127.0.0.1:%d'%server.server_port),patch.object(self.m,'admin_token',return_value='secret-token'):
            data,error=self.m.request('/api/provider-quotas')
        self.assertIsNone(data);self.assertEqual(error[0],302)
        self.assertEqual(asked,[])


if __name__=='__main__':
    unittest.main()
