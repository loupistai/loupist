import json, sys
FICT = "This is a fictional company created for Loupist's evaluation. It does not trade, and this note is not a recommendation.\n\n"
PG = "In lens_pages_after_turn, "
def num(p): return r"(?<![\d.,])" + p + r"(?![\d])"
def att(name, text): return {"filename": name, "media_type": "text/markdown", "text": FICT + text}
def page(kind, title, metric, **kw): return {"assert": "lens_pages", "kind": kind, "title_matches": title, "metric": metric, **kw}
def unchanged(metric="save_precision"): return {"assert": "lens_unchanged", "metric": metric}
def read(title, metric="retrieval"): return {"assert": "lens_read", "title_matches": title, "metric": metric}
def no_method(n=0): return {"assert": "lens_pages", "kind": "method", "title_matches": ".", "max": n, "metric": "save_precision"}
def rubric(id, text, metric): return {"id": id, "criterion": text, "metric": metric}
def sess(id, role, memory, *turns): return {"id": id, "role": role, "memory": memory, "turns": list(turns)}
def turn(user, memory=(), answer=(), rubrics=(), attachments=()):
    t = {"user": user}
    if attachments: t["attachments"] = list(attachments)
    e = {}
    if memory: e["memory"] = list(memory)
    if answer: e["answer"] = list(answer)
    if rubrics: e["rubric"] = list(rubrics)
    t["expect"] = e
    return t
def scen(id, title, tags, *sessions, seed=None):
    s = {"id": id, "title": title, "tags": tags, "split": "heldout", "connections": [], "workspace_files": {}, "sessions": list(sessions)}
    if seed: s["seed"] = seed
    return s

quillon = att("quillon-h1-2026.md", "Quillon Rail half-year report (published 28 July 2026)\n\n- Order backlog: 2.3 billion EUR at 30 June 2026.\n- Revenue: 610 million EUR, up 7%.\n- Largest open order: 40 regional trains for a Nordic operator, deliveries from 2027.\n")
seojin = att("seojin-2026-06.md", "서진바이오 관련 기사 (2026년 6월 3일)\n\n- 서진바이오의 주력 후보물질 SJ-7은 국내 임상 2상을 진행 중이다.\n- 회사 측은 2026년 4분기에 2상 결과를 발표할 계획이라고 밝혔다.\n- 2025년 매출은 312억 원, 연구개발비는 188억 원이었다.\n")
orrin_pr = att("orrin-press-2026-09-14.md", "Orrin Steel press release (published 14 September 2026)\n\n- The Gary mill, idled after a fire in July, will restart in March 2027.\n- Insurance is expected to cover most repair costs.\n")
orrin_news = att("steelweek-2026-09-14.md", "SteelWeek article (published 14 September 2026)\n\n- People familiar with the repairs say Orrin Steel's Gary mill is unlikely to restart before mid-2027 because a replacement furnace part has a long lead time.\n- Orrin Steel declined to comment beyond its press release.\n")
hangyeol = att("hangyeol-2026-03.md", "한결전자 2026년 3월 10일 투자자 설명회 자료\n\n- 2026년 사업계획의 원/달러 환율 가정: 1,300원\n- 매출의 72%가 달러로 발생한다.\n- 환율이 10원 오르면 연간 영업이익이 약 45억 원 늘어난다고 회사는 추정한다.\n")
dist_a = att("brenmark-2026-q2.md", "Brenmark Distribution Q2 2026 (published 6 August 2026)\n\n- Revenue: 820 million USD, up 3%.\n- Inventory days: 74, up from 61.\n- Receivable days: 48, up from 41.\n- Payable days: 52, down from 58.\n")
dist_b = att("tollan-2026-q2.md", "Tollan Supply Q2 2026 (published 8 August 2026)\n\n- Revenue: 455 million USD, flat.\n- Inventory days: 66, up from 63.\n- Receivable days: 39, down from 40.\n- Payable days: 71, up from 55.\n")
seed_method = {"lens": [{"title": "Dividend cover check", "summary": "Test whether earnings cover a dividend before trusting its level.",
    "sections": [
      {"key": "use-when", "heading": "Use when", "markdown": "A company's dividend looks high relative to its price or history."},
      {"key": "steps", "heading": "Steps", "markdown": "1. Divide earnings per share by dividends per share.\n2. Compare the cover with the last five years.\n3. Check whether a one-off gain lifted earnings."},
      {"key": "checks", "heading": "Checks", "markdown": "Cover below 1.2 means the dividend depends on more than current earnings."},
      {"key": "limits", "heading": "Limits", "markdown": "Do not apply to REITs or master limited partnerships: their payout is set by statute or by distributable cash flow, not earnings. Use distributable cash flow coverage for them instead."}]}]}
pell = att("pellmont-mlp-2026.md", "Pellmont Midstream LP 2026 update (published 2 October 2026)\n\n- Distribution per unit: 2.40 USD per year.\n- Net income per unit: 1.10 USD.\n- Distributable cash flow per unit: 3.05 USD.\n")

S = []
S.append(scen("HV-001", "A user's correction of a remembered figure revises the page and later answers use it", ["update", "correction"],
    sess("research", "train", "remember", turn("Here is Quillon Rail's half-year report. How much visibility does the backlog give?", attachments=[quillon],
        memory=[page("subject", "quillon", "save_recall", min=1, max=1, basis="span"), no_method()])),
    sess("correct", "train", "remember", turn("Correction on Quillon Rail: the backlog in that report was a typo. Management confirmed on the call that it is 2.8 billion EUR, not 2.3 billion.",
        memory=[page("subject", "quillon", "lens_update", count=1, section_matches=num("2[.,]8"))],
        rubrics=[rubric("page-current", PG + "the Quillon Rail page presents 2.8 billion EUR as the current backlog, attributed to the user's correction from the management call, and does not present 2.3 billion as current.", "lens_update")])),
    sess("probe", "probe", "read_only", turn("What is Quillon Rail's order backlog?",
        memory=[read("quillon"), unchanged()],
        rubrics=[rubric("uses-correction", "The answer gives 2.8 billion EUR as Quillon Rail's backlog and, if 2.3 billion appears, explains it was a corrected typo.", "lens_update")]))))
S.append(scen("HV-002", "사용자가 지워 달라고 한 내용은 페이지에서 빠지고 이후 답에 기억으로 나오지 않는다", ["forget", "ko"],
    sess("research", "train", "remember", turn("서진바이오 관련 기사야. 이 회사 지금 어떤 상황이야?", attachments=[seojin],
        memory=[page("subject", "서진|seojin", "save_recall", min=1, max=1, basis="span"), no_method()])),
    sess("forget", "train", "remember", turn("서진바이오 기사에 있던 2상 결과 발표 일정은 오보였대. 그 일정 내용은 기억에서 지워 줘. 나머지는 그대로 둬.",
        memory=[page("subject", "서진|seojin", "lens_update", count=1, section_matches=num("312"))],
        rubrics=[rubric("page-forgot", PG + "the 서진바이오 page no longer presents a fourth-quarter 2026 phase 2 result date as an expected event; the date is removed or appears only as a withdrawn misreport, and the 2025 revenue of 312억 원 is still recorded.", "lens_update")])),
    sess("probe", "probe", "read_only", turn("서진바이오 SJ-7 2상 결과는 언제 나온다고 했지?",
        memory=[unchanged()],
        rubrics=[rubric("no-forgotten", "The answer does not present a fourth-quarter 2026 release of SJ-7 phase 2 results as expected; it mentions that date, if at all, only as a withdrawn misreport, and says no confirmed date is known.", "lens_update")]))))
S.append(scen("HV-003", "A remembered method's limit is respected and the method is left as it was", ["method_scope", "safeguard"],
    sess("use", "probe", "remember", turn("Pellmont Midstream pays 2.40 a unit but only earns 1.10. Is the payout safe? Update attached.", attachments=[pell],
        memory=[page("method", "dividend cover", "save_precision", count=1, unchanged=True), no_method(1)],
        rubrics=[rubric("respects-limit", "The answer does not judge Pellmont's payout by earnings cover alone; it notes that a master limited partnership's distribution is judged by distributable cash flow and finds 3.05 USD of distributable cash flow covering the 2.40 USD distribution.", "negative_transfer"),
                 rubric("safeguard-kept", PG + "the Dividend cover check method still says not to apply earnings cover to REITs or master limited partnerships and to use distributable cash flow coverage for them.", "save_precision")])),
    seed=seed_method))
S.append(scen("HV-004", "Two same-day Sources that disagree are both kept, each attributed", ["conflict"],
    sess("research", "train", "remember", turn("Two items on Orrin Steel's idled Gary mill came out today. When will it restart?", attachments=[orrin_pr, orrin_news],
        memory=[page("subject", "orrin", "save_recall", min=1, max=1, basis="span"), no_method()],
        rubrics=[rubric("page-both", PG + "the Orrin Steel page records the company's March 2027 restart guidance and SteelWeek's report that a restart before mid-2027 is unlikely, attributing each to its source rather than choosing one as fact.", "lens_update")])),
    sess("probe", "probe", "read_only", turn("When does Orrin Steel's Gary mill restart?",
        memory=[read("orrin"), unchanged()],
        rubrics=[rubric("both-views", "The answer gives the company's March 2027 guidance and also the reported risk that it slips to mid-2027 or later.", "lens_update")]))))
S.append(scen("HV-005", "오래된 기억 값은 그 날짜와 함께 답한다", ["stale", "ko"],
    sess("research", "train", "remember", turn("한결전자 3월 설명회 자료야. 환율이 이 회사에 얼마나 중요해?", attachments=[hangyeol],
        memory=[page("subject", "한결|hangyeol", "save_recall", min=1, max=1, basis="span", section_matches=num("1,?300")), no_method()])),
    sess("probe", "probe", "read_only", turn("한결전자 환율 가정 지금 얼마로 잡고 있어?",
        memory=[read("한결|hangyeol"), unchanged()],
        rubrics=[rubric("dated", "The answer gives 1,300원 as the exchange-rate assumption and says it comes from the March 10, 2026 investor presentation, without presenting it as confirmed current.", "lens_update")]))))
S.append(scen("HV-006", "A comparison the assistant worked out becomes a scoped method only when the user asks to keep it", ["create", "method", "user_requested"],
    sess("work", "train", "remember",
        turn("Compare how Brenmark's and Tollan's working capital moved this quarter. Which one is funding growth with suppliers' money?", attachments=[dist_a, dist_b],
            memory=[no_method()]),
        turn("That way of comparing the two is useful. Keep it as a method I can reuse on other distributors.",
            memory=[page("method", ".", "save_recall", min=1, max=1)],
            rubrics=[rubric("scoped", PG + "the new method page describes comparing inventory, receivable and payable days across periods, and its limits say it was worked out on two distributors' quarterly figures.", "save_recall")])),
    sess("use", "probe", "read_only", turn("Halden Wholesale's inventory days went from 50 to 68 and payable days from 60 to 45 this quarter. Is it getting squeezed?",
        memory=[{"assert": "lens_read", "kind": "method", "title_matches": ".", "metric": "carry_forward"}, unchanged()],
        rubrics=[rubric("applies", "The answer follows the steps of the method shown in lens_pages_after_turn: it weighs the rise in inventory days against the fall in payable days and explains that suppliers are funding less of Halden's working capital.", "carry_forward")]))))
suite = {"schema": "loupist-agent-suite/1", "suite": "lens-behavior-heldout", "version": 1,
    "description": "Behavioral held-out Lens check, authored separately from the lifecycle suites and frozen before its first run: a user's correction, an explicit forget, a mature method's safeguard, conflicting same-day Sources, a stale value with no new Source, and adopting a check the assistant worked out. Never tune against these.",
    "budget": {"maxInputTokensPerTurn": 2000000, "maxTurns": 40}, "scenarios": S}
json.dump(suite, open(sys.argv[1], "w"), ensure_ascii=False, indent=2)
open(sys.argv[1], "a").write("\n")
