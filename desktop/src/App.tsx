import { useState } from 'react';
import { Sidebar } from './components/Sidebar';
import { Home } from './pages/Home';
import { Browse } from './pages/Browse';
import { Instances } from './pages/Instances';
import { Governance } from './pages/Governance';
import { Settings } from './pages/Settings';

type Tab = 'home' | 'browse' | 'instances' | 'governance' | 'settings';

const TABS: { id: Tab; label: string; icon: string }[] = [
  { id: 'home', label: 'Home', icon: '🏠' },
  { id: 'browse', label: 'Browse', icon: '🔍' },
  { id: 'instances', label: 'My Instances', icon: '📦' },
  { id: 'governance', label: 'Community Governance', icon: '🗳️' },
  { id: 'settings', label: 'Settings', icon: '⚙️' },
];

export default function App() {
  const [activeTab, setActiveTab] = useState<Tab>('home');

  return (
    <div className="flex h-screen w-screen overflow-hidden">
      <Sidebar tabs={TABS} activeTab={activeTab} onSelectTab={setActiveTab} />
      <main className="flex-1 overflow-y-auto p-6 surface">
        {activeTab === 'home' && <Home />}
        {activeTab === 'browse' && <Browse />}
        {activeTab === 'instances' && <Instances />}
        {activeTab === 'governance' && <Governance />}
        {activeTab === 'settings' && <Settings />}
      </main>
    </div>
  );
}
