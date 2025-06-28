---
# You can also start simply with 'default'
theme: default
# some information about your slides (markdown enabled)
title: Welcome to Slidev
info: |
  ## Slidev Starter Template
  Presentation slides for developers.

  Learn more at [Sli.dev](https://sli.dev)
# https://sli.dev/features/drawing
drawings:
  persist: false
# slide transition: https://sli.dev/guide/animations.html#slide-transitions
transition: malcolm
# enable MDC Syntax: https://sli.dev/features/mdc
mdc: true
---

# Malcolm Still

- 🧑‍💻 Senior Software Engineer @ Swordbreaker
- 🦀 Paid to write rust for 5 years

---
# Slide to give story hook
layout: center
---


# An interesting thing happened recently...


---
# This should be more frontmatter
layout: center
---

# The crab and the pufferfish

Applying OpenBSD's Secure Software Design Pattern in Rust

<!-- <div @click="$slidev.nav.next" class="mt-12 py-1" hover:bg="white op-10">
  Press Space for next page <carbon:arrow-right />
</div> -->

<!-- <div class="abs-br m-6 text-xl">
  <button @click="$slidev.nav.openInEditor()" title="Open in Editor" class="slidev-icon-btn">
    <carbon:edit />
  </button>
  <a href="https://github.com/slidevjs/slidev" target="_blank" class="slidev-icon-btn">
    <carbon:logo-github />
  </a>
</div> -->

<!--
The last comment block of each slide will be treated as slide notes. It will be visible and editable in Presenter Mode along with the slide. [Read more in the docs](https://sli.dev/guide/syntax.html#notes)
-->

---
# Slide to talk about rust background
layout: center
---

# Rust

Talk assumes familiarity with rust

---
# Facts about OpenBSD
# layout: center
---

# OpenBSD

- Unix-like operating system
- Specifically a BSD
  - See also: FreeBSD, NetBSD
- Forked from NetBSD in 1995
- Security focussed
- https://www.openbsd.org/innovations.html
  - privdrop
    - [pledge(2)](https://man.openbsd.org/pledge.2) 2015
    - [unveil(2)](https://man.openbsd.org/unveil.2) 2018
  - privsep