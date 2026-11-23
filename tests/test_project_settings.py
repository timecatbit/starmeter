import os,sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'lib'))
import swiftbar_projects


class ProjectSettingsTests(unittest.TestCase):
    def test_the_settings_file_is_in_the_first_configured_folder_with_its_tilde_expanded(self):
        home=os.path.expanduser('~')
        self.assertEqual(swiftbar_projects.settings_file(None),os.path.join(home,'.claude.json'))
        self.assertEqual(swiftbar_projects.settings_file('  '),os.path.join(home,'.claude.json'))
        self.assertEqual(swiftbar_projects.settings_file('~/.claude-work, /other'),os.path.join(home,'.claude-work','.claude.json'))
        self.assertEqual(swiftbar_projects.settings_file('/opt/claude'),'/opt/claude/.claude.json')


if __name__=='__main__':
    unittest.main()
