import type { Metadata } from 'next';
import './globals.css';

export const metadata: Metadata = {
  title: 'Curated Launcher Directory',
  description: 'A boutique, community-curated Minecraft mod directory.',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body className="min-h-screen bg-gray-50 text-gray-900 dark:bg-gray-900 dark:text-gray-100">
        <header className="border-b bg-white px-6 py-4 dark:border-gray-700 dark:bg-gray-800">
          <div className="mx-auto flex max-w-5xl items-center justify-between">
            <h1 className="text-xl font-bold">Curated Launcher</h1>
            <nav className="flex gap-4 text-sm">
              <a href="/" className="hover:underline">Directory</a>
            </nav>
          </div>
        </header>
        <main className="mx-auto max-w-5xl px-6 py-8">{children}</main>
      </body>
    </html>
  );
}
