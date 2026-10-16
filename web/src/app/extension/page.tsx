"use client";

export default function ExtensionPage() {
  return (
    <div className="min-h-screen bg-zinc-50 dark:bg-zinc-950">
      <header className="border-b border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900">
        <div className="max-w-3xl mx-auto px-4 py-4 flex items-center gap-3">
          <a href="/" className="flex items-center gap-2 hover:opacity-80 transition-opacity">
            <span className="text-2xl">🎙️</span>
            <h1 className="text-xl font-bold text-zinc-900 dark:text-zinc-100">Lumen Pod</h1>
          </a>
        </div>
      </header>

      <main className="max-w-3xl mx-auto px-4 py-8">
        {/* Hero */}
        <div className="text-center mb-10">
          <div className="inline-flex items-center justify-center w-20 h-20 rounded-2xl bg-gradient-to-br from-blue-500 to-indigo-600 shadow-lg mb-4">
            <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="12" cy="12" r="10" />
              <polygon points="10 8 16 12 10 16 10 8" />
            </svg>
          </div>
          <h2 className="text-2xl font-bold text-zinc-900 dark:text-zinc-100 mb-2">
            Lumen Pod Chrome Extension
          </h2>
          <p className="text-zinc-500 dark:text-zinc-400 max-w-md mx-auto">
            在 YouTube 页面直接查看「读完就够」深度长文、智能章节、实时字幕和深度解读
          </p>
        </div>

        {/* Download */}
        <div className="bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 p-6 mb-8">
          <div className="flex items-center justify-between mb-4">
            <div>
              <h3 className="text-lg font-semibold text-zinc-900 dark:text-zinc-100">下载扩展</h3>
              <p className="text-sm text-zinc-500 dark:text-zinc-400 mt-0.5">当前版本 v0.2.0 · Chrome / Edge / Arc（含深度长文与一键提交）</p>
            </div>
            <a
              href="/static/lumen-pod-extension-v0.2.0.zip"
              download
              className="inline-flex items-center gap-2 px-5 py-2.5 bg-blue-500 hover:bg-blue-600 text-white font-medium rounded-lg transition-colors shadow-sm"
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                <polyline points="7 10 12 15 17 10" />
                <line x1="12" y1="15" x2="12" y2="3" />
              </svg>
              下载 ZIP
            </a>
          </div>
        </div>

        {/* Install Guide */}
        <div className="bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 p-6 mb-8">
          <h3 className="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-4">安装指南</h3>

          <div className="space-y-6">
            <Step num={1} title="下载并解压">
              <p>点击上方「下载 ZIP」按钮，将文件保存到本地。</p>
              <p>解压 ZIP 文件到一个固定的文件夹（不要删除，Chrome 需要持续读取）。</p>
            </Step>

            <Step num={2} title="打开 Chrome 扩展管理页">
              <p>在 Chrome 地址栏输入 <code className="px-1.5 py-0.5 bg-zinc-100 dark:bg-zinc-800 rounded text-sm">chrome://extensions</code> 并回车。</p>
              <p>或通过菜单：<span className="text-zinc-600 dark:text-zinc-400">⋮ → 扩展程序 → 管理扩展程序</span></p>
            </Step>

            <Step num={3} title="启用开发者模式">
              <p>在页面右上角找到「<strong>开发者模式</strong>」开关，将其打开。</p>
            </Step>

            <Step num={4} title="加载扩展">
              <p>点击左上角出现的「<strong>加载已解压的扩展程序</strong>」按钮。</p>
              <p>选择你解压出来的文件夹（包含 <code className="px-1.5 py-0.5 bg-zinc-100 dark:bg-zinc-800 rounded text-sm">manifest.json</code> 的那个目录）。</p>
            </Step>

            <Step num={5} title="配置 API 连接">
              <p>打开任意 YouTube 视频页面，点击浏览器工具栏中的 Lumen Pod 图标。</p>
              <p>在侧边栏点击右上角 ⚙️ 图标，填入：</p>
              <ul className="list-disc ml-5 mt-1 space-y-1">
                <li><strong>API 地址</strong>：你的 Lumen Pod 服务地址（如 <code className="px-1.5 py-0.5 bg-zinc-100 dark:bg-zinc-800 rounded text-sm">http://pod.lumenopen.com</code>）</li>
                <li><strong>API Token</strong>：在服务端设置的 <code className="px-1.5 py-0.5 bg-zinc-100 dark:bg-zinc-800 rounded text-sm">LUMEN_POD_API_TOKEN</code></li>
              </ul>
            </Step>

            <Step num={6} title="开始使用">
              <p>打开一个已收录的 YouTube 视频页面，右下角会出现蓝色指示器。</p>
              <p>点击工具栏图标打开侧边栏，即可查看摘要、章节、字幕和问答。</p>
              <p>字幕会实时跟踪视频播放进度，并高亮当前句子。</p>
            </Step>
          </div>
        </div>

        {/* Features */}
        <div className="bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 p-6">
          <h3 className="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-4">功能特性</h3>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <Feature icon="✍️" title="「读完就够」深度长文" desc="具体画面开场、还原推理链、三段式金句与书影音延伸清单" />
            <Feature icon="⏱️" title="内联时间戳跳转" desc="点击文章与精读中的 [MM:SS] 时间戳直接跳转视频对应画面" />
            <Feature icon="📝" title="AI 摘要与一键提交" desc="未收录视频支持在侧边栏一键提交至 Lumen Pod 队列" />
            <Feature icon="📑" title="智能章节" desc="章节导航、摘要、深度解读展开阅读" />
            <Feature icon="💬" title="实时字幕" desc="逐句高亮跟踪、点击跳转、多说话人标识" />
            <Feature icon="❓" title="问答与暗色模式" desc="AI 问答提炼，跟随系统自动切换明暗主题" />
          </div>
        </div>
      </main>
    </div>
  );
}

function Step({ num, title, children }: { num: number; title: string; children: React.ReactNode }) {
  return (
    <div className="flex gap-4">
      <div className="flex-shrink-0 w-8 h-8 rounded-full bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 flex items-center justify-center font-bold text-sm">
        {num}
      </div>
      <div className="flex-1 min-w-0">
        <h4 className="font-medium text-zinc-800 dark:text-zinc-200 mb-1">{title}</h4>
        <div className="text-sm text-zinc-600 dark:text-zinc-400 space-y-1 leading-relaxed">
          {children}
        </div>
      </div>
    </div>
  );
}

function Feature({ icon, title, desc }: { icon: string; title: string; desc: string }) {
  return (
    <div className="flex items-start gap-3 p-3 rounded-lg bg-zinc-50 dark:bg-zinc-800/50">
      <span className="text-xl">{icon}</span>
      <div>
        <h5 className="text-sm font-medium text-zinc-800 dark:text-zinc-200">{title}</h5>
        <p className="text-xs text-zinc-500 dark:text-zinc-400 mt-0.5">{desc}</p>
      </div>
    </div>
  );
}
