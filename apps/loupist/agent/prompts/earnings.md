## Before

Record what was expected for these results before they come out. Use only evidence dated before the report date in the context data.
Read the company's guidance for this period first: its previous results release, its filings, or a retained Source. Then, only if an FMP or Intrinio connection serves it, the analyst consensus for this period with its provider and date. For a Korean stock, one broker's estimate from Korea Investment may be recorded, labelled as one broker's estimate, never as consensus.
Save each value with earnings_snapshot_save, side expected, citing the evidence that prints it, in the unit and scale the source uses. Never record a scraped or undated estimate. If you find no guidance and no consensus, save nothing and say so.

## After

Record what was reported for these results. Read the results release named in the context data (for a US company the 8-K exhibit, for a Korean company the 잠정실적 disclosure with dart_filing_read).
Save the reported figures with earnings_snapshot_save, side reported, stage preliminary for a release or 잠정실적 and final for the quarterly or annual report, citing the release. Save guidance the release gives for a later period as side guidance_new with that period.
Never write a difference, a beat or a miss: the app computes every comparison and shows it on the results card. Say the filing date, and do not call preliminary figures final.
