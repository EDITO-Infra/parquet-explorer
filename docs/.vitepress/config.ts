import { defineConfig } from "vitepress"

export default defineConfig({
  title: "Parquet Explorer",
  description: "Architecture and developer documentation for Parquet Explorer",

  cleanUrls: true,

  themeConfig: {
    nav: [
      { text: "Documentation", link: "/" },
      { text: "Architecture", link: "/architecture" },
      { text: "Parquet", link: "/parquet" }
    ],

    sidebar: [
      {
        text: "Overview",
        items: [
          { text: "Documentation", link: "/" },
          { text: "Installation", link: "/installation" },
          { text: "Architecture", link: "/architecture" },
          { text: "API", link: "/API" }
        ]
      },
      {
        text: "Backend",
        items: [
          { text: "Engine", link: "/engine" },
          { text: "Parquet", link: "/parquet" }
        ]
      },
      {
        text: "Analysis",
        items: [
          { text: "Analysis", link: "/analysis" },
          { text: "Analysis API", link: "/ANALYSIS_API" }
        ]
      },
      {
        text: "Frontend",
        items: [
          { text: "Frontend", link: "/frontend" }
        ]
      }
    ],

    search: {
      provider: "local"
    },

    outline: {
      level: [2, 3]
    }
  }
})
