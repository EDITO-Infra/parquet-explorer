import { defineConfig } from "vitepress"

export default defineConfig({
  title: "Parquet Viewer",
  description: "Architecture and developer documentation for Parquet Viewer",

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
          { text: "Architecture", link: "/architecture" }
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
