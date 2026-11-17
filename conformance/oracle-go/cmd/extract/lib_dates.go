package main

import (
	"fmt"
	"os"
	"strings"
)

func init() {
	libRegistry = append(libRegistry, handGroup("lib-dates", dateCases()))
	libRegistry = append(libRegistry, handGroup("lib-dates-clock", dateClockCases()))
	if os.Getenv("UDDIN_DATES_TZ") != "" {
		libRegistry = append(libRegistry, handGroup("lib-dates-tz", dateZoneCases()))
	}
}

// calls prints each expression in its own try, so that an error value, a type error and a result are all shown.
func calls(exprs ...string) string {
	var b strings.Builder
	for _, e := range exprs {
		b.WriteString(tryPrint(e))
	}
	return b.String()
}

// dateInputs are texts to read as dates: good ones in each of the four layouts, boundaries of the calendar,
// and every way a date can be wrong.
var dateInputs = []string{
	"2023-12-25T10:30:00Z", "2023-12-25T10:30:00+00:00", "2023-12-25T10:30:00+07:00", "2023-12-25T10:30:00-05:00", "2023-12-25T10:30:00+05:30",
	"2023-12-25T10:30:00+14:00", "2023-12-25T10:30:00-12:00", "2023-12-25T10:30:00+24:00", "2023-12-25T10:30:00-23:59", "2023-12-25T10:30:00+23:60",
	"2023-12-25T10:30:00+25:00", "2023-12-25T10:30:00-00:00", "2023-12-25T10:30:00+0530", "2023-12-25T10:30:00+05", "2023-12-25T10:30:00 +05:30",
	"2024-02-29T00:00:00Z", "2023-02-28T23:59:59Z", "2024-02-29T23:59:59Z", "2100-02-28T00:00:00Z", "2000-02-29T12:00:00Z", "1900-02-28T12:00:00Z",
	"0001-01-01T00:00:00Z", "0001-01-01T00:00:00+07:00", "0000-01-01T00:00:00Z", "9999-12-31T23:59:59Z", "9999-12-31T23:59:59-07:00", "1970-01-01T00:00:00Z", "1969-12-31T23:59:59Z",
	"2023-12-25T10:30:00.5Z", "2023-12-25T10:30:00.123456789Z", "2023-12-25T10:30:00.1234567891234Z", "2023-12-25T10:30:00,5Z", "2023-12-25T10:30:00.Z", "2023-12-25T10:30:00.5",
	"2023-12-25T10:30:00.9999999999+07:00", "2023-12-25T10:30:00.000Z",
	"2023-12-25T10:30:00", "2023-12-25T10:30:00.25", "2023-12-25", "2023-12-25 10:30:00", "2023-12-25 10:30:00.75", "2023-12-31", "2023-01-01", "2023-12-25T00:00:00", "2023-12-25T23:59:59",
	"2023-03-12T02:30:00Z", "2023-03-12T06:59:59Z", "2023-03-12T07:00:00Z", "2023-11-05T05:59:59Z", "2023-11-05T06:00:00Z", "2023-03-26T00:59:59Z", "2023-03-26T01:00:00Z", "2023-10-29T00:59:59Z", "2023-10-29T01:00:00Z",
	"2023-03-12T01:30:00-05:00", "2023-03-12T03:30:00-04:00", "2023-03-12T02:30:00-05:00", "2023-11-05T01:30:00-04:00", "2023-11-05T01:30:00-05:00", "2023-03-26T01:30:00+00:00", "2023-03-26T02:30:00+01:00", "2023-10-29T01:30:00+01:00", "2023-10-29T01:30:00+00:00",
	"", " ", "x", "nope", "2023", "2023-12", "2023-1-5", "2023-12-5", "23-12-25", "12/25/2023", "2023/12/25", "20231225", "2023-12-25T", "2023-12-25T10", "2023-12-25T10:30", "2023-12-25T10:30:0", "2023-12-25T1:30:00Z",
	"2023-12-25t10:30:00Z", "2023-12-25T10:30:00z", "2023-12-25  10:30:00", " 2023-12-25", "2023-12-25 ", "2023-12-25T10:30:00Z ", "2023-12-25T10:30:00Zextra", "2023-12-25extra", "2023-12-25 10:30:00 extra",
	"2023-13-01", "2023-00-10", "2023-12-00", "2023-12-32", "2023-02-29", "2023-02-30", "2023-04-31", "2024-02-30", "2023-12-25T24:00:00Z", "2023-12-25T25:00:00Z", "2023-12-25T10:60:00Z", "2023-12-25T10:30:60Z",
	"2023-12-25T-1:30:00Z", "2023-12-25T10:-1:00Z", "20x3-12-25", "２０２３-12-25", "2023-12-25T10:30:00Z\"quoted\"", "2023\\12\\25", "2023-12-25\t", "2023-12-25\n", "é2023-12-25", "2023-12-25é", "日本",
	"2023-12-25T10:30:00+0530Z", "2023-12-25T10:30:00+05:3", "2023-12-25T10:30:00+5:30", "2023-12-25T10:30:00*05:30", "2023-12-25T10:30:00+aa:bb",
	"2023-12-25T10:30:00" + strings.Repeat("0", 200), strings.Repeat("a", 600), strings.Repeat("é", 300),
}

func dateCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, src}) }
	good := []string{"2023-12-25T10:30:00Z", "2023-12-25T10:30:00+07:00", "2023-12-25T10:30:00-05:00", "2023-12-25T10:30:00+05:30", "2024-02-29T23:59:59Z", "0001-01-01T00:00:00Z", "9999-12-31T23:59:59Z", "2023-12-25T10:30:00.123456789Z", "2023-12-31T23:59:59.9Z"}

	// Reading a date: every input through each builtin that reads one, the date alone in the second slot.
	for i, in := range dateInputs {
		q := uq(in)
		add(fmt.Sprintf("read/%03d", i), calls(
			"date_compare("+q+", \"2023-12-25\")",
			"date_diff("+q+", \"2023-12-24\")",
			"date_between("+q+", \"2000-01-01\", \"2100-01-01\")",
			"date_add("+q+", \"days\", 1.0)",
			"date_subtract("+q+", \"hours\", 1.0)",
			"date_format("+q+", \"YYYY-MM-DD hh:mm:ss\")",
		))
	}

	// date_format: the short names of the builtin, and full layouts, over good dates.
	layouts := []string{
		"YYYY-MM-DD", "YYYY-MM-DD hh:mm:ss", "DD/MM/YYYY", "EE, DD NN YYYY", "ee nn DD", "hh:mm", "YYYY", "ss", "YYYYYYYY", "MMM", "DDD", "hhh", "NNN nnn EEE eee", "",
		"2006-01-02", "2006-01-02T15:04:05Z07:00", "2006-01-02 15:04:05.000", "2006-01-02 15:04:05.999999999", "02 Jan 06 15:04 MST", "Mon Jan _2 15:04:05 MST 2006", "Monday, 02-Jan-06 15:04:05 MST",
		"3:04PM", "3:04pm", "03:04:05 PM", "Jan 2, 2006 at 3:04pm (MST)", "-0700", "-07:00", "-07", "-070000", "-07:00:00", "Z0700", "Z07:00", "Z07", "Z070000", "Z07:00:00",
		".0", ".00", ".000000000", ".9", ".99", ".999999999", ",000", ",999", "05.000", "05.999", "05,000", "5.00", "__2 002", "_2", "2", "1", "01", "06", "15", "5", "4", "3", "Jan", "January", "Mon", "Monday", "MST", "PM", "pm",
		"x", "Janx", "Janu", "January2", "Mondayx", "Mont", "2006x", "20066", "200", "_2006", "a1b2c3", "1:2:3", "06-1-2", "15h04m05s", "%Y-%m-%d", "[YYYY]", "'YYYY'", "\"YYYY\"", "YYYY\\MM", "é YYYY ü", "日本 YYYY",
	}
	for i, in := range good {
		var exprs []string
		for _, l := range layouts {
			exprs = append(exprs, "date_format("+uq(in)+", "+uq(l)+")")
		}
		add(fmt.Sprintf("format/%d", i), calls(exprs...))
	}
	add("format/null_for_dates_that_do_not_parse", calls("date_format(\"2023-12-25\", \"YYYY\")", "date_format(\"x\", \"YYYY\")", "date_format(\"\", \"\")", "date_format(\"2023-12-25T10:30:00\", \"YYYY\")", "date_format(\"2023-12-25T10:30:00Z\", \"\")", "typeof(date_format(\"x\", \"YYYY\"))", "date_format(\"2023-12-25T10:30:00Z\", 5)", "date_format(5, \"YYYY\")", "date_format(null, null)", "date_format([], \"x\")"))

	// Moving a date: units, spellings of units, amounts. The amount is a float; an integer is refused.
	amounts := []string{"0.0", "1.0", "-1.0", "1.9", "-1.9", "0.5", "-0.5", "7.0", "12.0", "24.0", "31.0", "365.0", "366.0", "1000.0", "100000.0", "1e10", "-1e10", "1e15", "1e18", "-1e18", "9e18", "1e19", "-1e19", "1e300", "(1e300 * 1e300)", "-(1e300 * 1e300)", "((1e300 * 1e300) - (1e300 * 1e300))"}
	units := []string{"years", "year", "months", "month", "days", "day", "hours", "hour", "minutes", "minute", "seconds", "second", "weeks", "week", "milliseconds", "nanoseconds", "Years", "DAYS", "Hours", "MiNuTeS", "mİnutes", "dayS", "day ", " day", "", "d", "yr", "ms", "y", "h", "m", "s"}
	for _, fn := range []string{"date_add", "date_subtract"} {
		for i, in := range good[:5] {
			var exprs []string
			for _, u := range units {
				exprs = append(exprs, fn+"("+uq(in)+", "+uq(u)+", 1.0)")
			}
			add(fmt.Sprintf("%s/units_%d", fn, i), calls(exprs...))
		}
		for i, in := range good {
			var exprs []string
			for _, a := range amounts {
				for _, u := range []string{"years", "months", "days", "hours", "minutes", "seconds"} {
					exprs = append(exprs, fn+"("+uq(in)+", "+uq(u)+", "+a+")")
				}
			}
			add(fmt.Sprintf("%s/amounts_%d", fn, i), calls(exprs...))
		}
		add(fn+"/integer_amounts", calls(fn+"(\"2023-12-25\", \"days\", 1)", fn+"(\"2023-12-25\", \"days\", 0)", fn+"(\"2023-12-25\", \"days\", -3)", fn+"(\"2023-12-25\", \"days\", \"1\")", fn+"(\"2023-12-25\", \"days\", null)", fn+"(\"2023-12-25\", \"days\", true)", fn+"(\"2023-12-25\", \"days\", [1])", fn+"(\"2023-12-25\", \"days\", {a: 1})", fn+"(\"2023-12-25\", 5, 1.0)", fn+"(5, \"days\", 1.0)", fn+"(\"2023-12-25\", [], 1.0)", fn+"(null, \"days\", 1.0)", fn+"(\"x\", 5, 1.0)", fn+"(\"x\", \"days\", \"y\")", fn+"(\"2023-12-25\", \"fortnights\", 1)"))
		add(fn+"/arity", calls(fn+"()", fn+"(\"2023-12-25\")", fn+"(\"2023-12-25\", \"24h\")", fn+"(\"2023-12-25\", \"hours\", 24.0)", fn+"(\"2023-12-25\", \"hours\", 24.0, 1.0)", fn+"(\"2023-12-25\", \"days\", 1.0, 1, 2)"))
		add(fn+"/month_ends", calls(fn+"(\"2023-01-31\", \"months\", 1.0)", fn+"(\"2023-03-31\", \"months\", -1.0)", fn+"(\"2024-02-29\", \"years\", 1.0)", fn+"(\"2024-02-29\", \"years\", -1.0)", fn+"(\"2023-12-31\", \"days\", 1.0)", fn+"(\"2024-12-31\", \"days\", 1.0)", fn+"(\"2023-12-31\", \"months\", 2.0)", fn+"(\"2023-10-31\", \"months\", 4.0)", fn+"(\"2023-12-31\", \"months\", 12.0)", fn+"(\"2023-12-31\", \"months\", 13.0)", fn+"(\"2023-12-31\", \"months\", 1200.0)", fn+"(\"9999-12-31\", \"days\", 1.0)", fn+"(\"0001-01-01\", \"days\", 1.0)", fn+"(\"0001-01-01\", \"days\", -1.0)", fn+"(\"0001-01-01\", \"years\", -1.0)", fn+"(\"9999-12-31T23:59:59Z\", \"seconds\", 1.0)", fn+"(\"0001-01-01T00:00:00Z\", \"seconds\", -1.0)", fn+"(\"2023-12-25T10:30:00.7Z\", \"seconds\", 1.0)", fn+"(\"2023-12-25T10:30:00.7+05:30\", \"minutes\", 90.0)"))
		add(fn+"/offsets", calls(fn+"(\"2023-12-25T10:30:00+05:30\", \"days\", 1.0)", fn+"(\"2023-12-25T10:30:00+05:30\", \"hours\", 24.0)", fn+"(\"2023-12-25T23:30:00-05:00\", \"hours\", 1.0)", fn+"(\"2023-12-25T10:30:00+07:00\", \"hours\", 1.0)", fn+"(\"2023-12-25T10:30:00+07:00\", \"months\", 6.0)", fn+"(\"2023-12-25T10:30:00+00:00\", \"hours\", 1.0)", fn+"(\"2023-12-25T10:30:00Z\", \"hours\", 1.0)", fn+"(\"2023-12-25T10:30:00-00:00\", \"hours\", 1.0)", fn+"(\"2023-12-25T10:30:00+14:00\", \"days\", 1.0)", fn+"(\"2023-12-25T10:30:00+24:00\", \"days\", 1.0)"))
	}

	// date_diff: every unit, every pair of the good dates, two and three and four arguments.
	diffUnits := []string{"nanoseconds", "nanosecond", "microseconds", "microsecond", "milliseconds", "millisecond", "seconds", "second", "minutes", "minute", "hours", "hour", "days", "day", "Days", "HOURS", "weeks", "years", "", "x", "ms", "s"}
	for i, a := range good {
		var exprs []string
		for j, b := range good {
			if (i+j)%2 == 0 || i == j {
				exprs = append(exprs, "date_diff("+uq(a)+", "+uq(b)+")")
			}
			if (i+j)%3 == 0 {
				for _, u := range diffUnits {
					exprs = append(exprs, "date_diff("+uq(a)+", "+uq(b)+", "+uq(u)+")")
				}
			}
		}
		add(fmt.Sprintf("diff/%d", i), calls(exprs...))
	}
	add("diff/arity_and_types", calls("date_diff()", "date_diff(\"2023-12-25\")", "date_diff(1, 2)", "date_diff(\"2023-12-25\", 2)", "date_diff(\"2023-12-25\", \"2023-12-24\", 3)", "date_diff(\"2023-12-25\", \"2023-12-24\", \"days\", \"ignored\")", "date_diff(\"2023-12-25\", \"2023-12-24\", \"days\", 1, 2, 3)", "date_diff(null, \"x\")", "date_diff(\"x\", null)", "date_diff(\"x\", \"y\")", "date_diff(\"2023-12-25\", \"y\")", "date_diff(\"2023-12-25\", \"y\", \"days\")", "date_diff(\"2023-12-25\", \"2023-12-24\", null)", "date_diff(\"2023-12-25\", \"2023-12-24\", [])"))
	add("diff/extremes", calls("date_diff(\"9999-12-31T23:59:59Z\", \"0001-01-01T00:00:00Z\")", "date_diff(\"0001-01-01T00:00:00Z\", \"9999-12-31T23:59:59Z\", \"nanoseconds\")", "date_diff(\"2262-04-11T23:47:16Z\", \"1970-01-01T00:00:00Z\", \"nanoseconds\")", "date_diff(\"2262-04-11T23:47:17Z\", \"1970-01-01T00:00:00Z\", \"nanoseconds\")", "date_diff(\"1970-01-01T00:00:00Z\", \"2262-04-11T23:47:17Z\", \"nanoseconds\")", "date_diff(\"2292-04-11T00:00:00Z\", \"2000-01-01T00:00:00Z\", \"days\")", "date_diff(\"2023-12-25T10:30:00.9Z\", \"2023-12-25T10:30:00Z\", \"seconds\")", "date_diff(\"2023-12-25T10:30:00Z\", \"2023-12-25T10:30:00.9Z\", \"seconds\")", "date_diff(\"2023-12-25T10:30:00Z\", \"2023-12-25T10:30:00.9Z\", \"milliseconds\")", "date_diff(\"2023-12-25T10:30:00Z\", \"2023-12-25T10:30:00.9Z\")", "date_diff(\"2023-12-25T10:30:00.123456789Z\", \"2023-12-25T10:30:00Z\", \"nanoseconds\")", "date_diff(\"2023-12-25T10:30:00.123456789Z\", \"2023-12-25T10:30:00Z\", \"microseconds\")"))
	add("diff/offsets", calls("date_diff(\"2023-12-25T10:30:00+05:30\", \"2023-12-25T10:30:00Z\", \"minutes\")", "date_diff(\"2023-12-25T10:30:00Z\", \"2023-12-25T10:30:00+05:30\", \"hours\")", "date_diff(\"2023-12-25T10:30:00+07:00\", \"2023-12-25T03:30:00Z\")", "date_diff(\"2023-12-25\", \"2023-12-25T00:00:00+07:00\", \"hours\")"))
	add("diff/is_not_an_integer_for_the_reference", "x = date_diff(\"2023-12-25\", \"2023-12-24\", \"days\")\nprint(x)\nprint(str(x))\nprint([x])\nprint(len(str(x)))\n")

	// date_between and date_compare over every pair of a few dates.
	trio := []string{"2023-12-24", "2023-12-25", "2023-12-26", "2023-12-25T10:30:00Z", "2023-12-25T10:30:00+07:00", "2023-12-25T03:30:00Z"}
	var exprs []string
	for _, a := range trio {
		for _, b := range trio {
			exprs = append(exprs, "date_compare("+uq(a)+", "+uq(b)+")")
			for _, c := range trio {
				exprs = append(exprs, "date_between("+uq(a)+", "+uq(b)+", "+uq(c)+")")
			}
		}
	}
	add("compare_between/pairs", calls(exprs...))
	add("compare/arity_and_types", calls("date_compare()", "date_compare(\"2023-12-25\")", "date_compare(\"2023-12-25\", \"2023-12-24\", \"x\")", "date_compare(1, 2)", "date_compare(\"2023-12-25\", 2)", "date_compare(null, null)", "date_compare(\"x\", \"2023-12-25\")", "date_compare(\"2023-12-25\", \"x\")", "date_compare([], [])", "date_compare(\"\", \"\")"))
	add("between/arity_and_types", calls("date_between()", "date_between(\"2023-12-25\")", "date_between(\"2023-12-25\", \"2023-12-24\")", "date_between(\"2023-12-25\", \"2023-12-24\", \"2023-12-26\", \"x\")", "date_between(1, 2, 3)", "date_between(\"2023-12-25\", 2, 3)", "date_between(\"2023-12-25\", \"2023-12-24\", 3)", "date_between(\"x\", \"2023-12-24\", \"2023-12-26\")", "date_between(\"2023-12-25\", \"x\", \"2023-12-26\")", "date_between(\"2023-12-25\", \"2023-12-24\", \"x\")", "date_between(\"x\", \"y\", \"z\")", "date_between(null, null, null)"))
	add("between/boundaries", calls("date_between(\"2023-12-25\", \"2023-12-25\", \"2023-12-25\")", "date_between(\"2023-12-25T00:00:00Z\", \"2023-12-25\", \"2023-12-26\")", "date_between(\"2023-12-26\", \"2023-12-25\", \"2023-12-26\")", "date_between(\"2023-12-26T00:00:00.1Z\", \"2023-12-25\", \"2023-12-26\")", "date_between(\"2023-12-25\", \"2023-12-26\", \"2023-12-24\")", "date_between(\"2023-12-25T10:00:00+07:00\", \"2023-12-25T03:00:00Z\", \"2023-12-25T03:00:00Z\")"))

	// The two builtins whose argument counts do not fit what they need.
	add("parse/always_refused", calls("date_parse()", "date_parse(\"2023-12-25\")", "date_parse(\"2023-12-25\", \"2006-01-02\")", "date_parse(1)", "date_parse(null)", "date_parse(\"x\", \"y\", \"z\")", "typeof(date_parse(\"2023-12-25\"))"))
	add("format_new/always_refused", calls("date_format_new()", "date_format_new(\"2023-12-25\")", "date_format_new(\"2023-12-25\", \"2006-01-02\")", "date_format_new(\"2023-12-25\", \"2006-01-02\", \"Jan 2\")", "date_format_new(1, 2)", "date_format_new(null, null)", "typeof(date_format_new(\"a\", \"b\"))"))
	add("format/arity", calls("date_format()", "date_format(\"2023-12-25T10:30:00Z\")", "date_format(\"2023-12-25T10:30:00Z\", \"YYYY\", \"x\")"))
	add("compare/arity_extra", calls("date_compare(\"a\", \"b\", \"c\", \"d\")", "date_between(\"a\", \"b\", \"c\", \"d\")"))

	// Errors are values: they can be kept, passed on and printed.
	add("errors/are_values", "x = date_add(\"nope\", \"days\", 1.0)\nprint(typeof(x))\nprint(x)\nprint(len(str(x)))\ny = [date_diff(\"a\", \"b\"), date_compare(\"c\", \"d\")]\nprint(len(y))\nprint(y)\nif (x) then:\n    print(1)\nend\n")
	add("errors/text_is_cut", "x = date_add(\"a\" * 700, \"days\", 1.0)\nprint(len(str(x)))\nprint(typeof(x))\ny = date_diff(\"é\" * 400, \"2023-12-25\")\nprint(len(str(y)))\n")
	add("errors/quotes_in_the_text", calls("date_add(\"a\\\"b\\\\c\", \"days\", 1.0)", "date_add(\"a\\tb\", \"days\", 1.0)", "date_add(\"é日\", \"days\", 1.0)", "date_add("+uq("\x7f")+", \"days\", 1.0)", "date_compare(\"\\r\\n\", \"x\")", "date_compare("+uq("a\u00e9")+", \"x\")", "date_add(\"\\\"\\\"\", \"days\", 1.0)"))
	return cs
}

// dateClockCases name the clock, so a comparison looks at how they end and not at what they print.
func dateClockCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, src}) }
	add("now/shape", "x = date_now()\nprint(typeof(x))\nprint(len(x) >= 20)\nprint(len(x) <= 25)\ny = time_now()\nprint(typeof(y))\nprint(y > 1700000000000)\nprint(y < 4000000000000)\n")
	add("now/arity", calls("date_now(1)", "date_now(1, 2)", "date_now(null)", "time_now(1)", "time_now(\"x\", 2)", "time_now([])"))
	add("now/used_as_dates", "x = date_now()\nprint(typeof(date_add(x, \"days\", 1.0)))\nprint(date_compare(x, date_add(x, \"seconds\", -1.0)))\nprint(date_diff(date_add(x, \"hours\", 1.0), x, \"hours\"))\nprint(date_between(x, \"2000-01-01\", \"2999-01-01\"))\nprint(typeof(date_format(x, \"YYYY\")))\n")
	add("now/time_is_increasing", "a = time_now()\nb = time_now()\nprint(b >= a)\nprint(typeof(a))\n")
	add("now/two_calls_close", "a = date_now()\nb = date_now()\nx = date_diff(b, a, \"seconds\")\nprint(str(x) == \"0\" or str(x) == \"1\")\n")
	return cs
}

// dateZoneCases are for the zones of the host: the same cases are run under several settings of TZ, each by both
// implementations (conformance/dates-tz.sh); the text of a case does not depend on the zone.
func dateZoneCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, src}) }
	instants := []string{
		"2023-03-12T01:59:59-05:00", "2023-03-12T03:00:00-04:00", "2023-03-12T07:00:00Z", "2023-11-05T01:30:00-04:00", "2023-11-05T01:30:00-05:00", "2023-11-05T06:30:00Z",
		"2023-03-26T00:59:59+00:00", "2023-03-26T02:00:00+01:00", "2023-10-29T01:30:00+01:00", "2023-10-29T01:30:00+00:00", "2023-10-01T02:00:00+10:30", "2023-04-02T01:59:59+11:00", "2023-04-02T02:30:00+10:30",
		"2023-06-15T12:00:00+05:30", "2023-12-25T10:30:00+07:00", "2023-12-25T10:30:00-03:00", "2023-12-25T10:30:00+13:45", "2023-12-25T10:30:00+12:45", "2023-01-15T12:00:00-05:00", "2023-07-15T12:00:00-04:00", "2023-07-15T12:00:00+01:00", "2023-01-15T12:00:00+00:00",
		"2023-12-25T10:30:00Z", "2023-12-25T10:30:00", "2023-12-25", "2024-02-29T12:00:00-05:00", "2024-02-29T12:00:00+01:00", "1970-01-01T00:00:00+00:00", "1900-01-01T00:00:00+00:00", "0001-01-01T00:00:00+00:00", "9999-12-31T23:59:59+00:00",
	}
	for i, in := range instants {
		q := uq(in)
		var e []string
		for _, l := range []string{"YYYY-MM-DD hh:mm:ss MST", "2006-01-02T15:04:05Z07:00 -0700", "MST -07:00 Z0700", "Mon Jan _2 15:04:05 MST 2006"} {
			e = append(e, "date_format("+q+", "+uq(l)+")")
		}
		for _, u := range []string{"days", "months", "years", "hours", "minutes", "seconds"} {
			for _, a := range []string{"1.0", "-1.0", "7.0", "24.0", "13.0", "-12.0", "100.0"} {
				e = append(e, "date_add("+q+", "+uq(u)+", "+a+")", "date_subtract("+q+", "+uq(u)+", "+a+")")
			}
		}
		e = append(e, "date_diff("+q+", \"2023-01-01T00:00:00Z\", \"hours\")", "date_compare("+q+", \"2023-06-01T00:00:00Z\")")
		add(fmt.Sprintf("zone/%02d", i), calls(e...))
	}
	return cs
}
