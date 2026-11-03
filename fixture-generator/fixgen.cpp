// qt-fixgen — QuantLib golden fixture generator (ADR-0001).
//
// Baseline: Ubuntu 24.04 apt libquantlib0-dev 1.33-1 (upstream QuantLib 1.33),
// g++ -O2 -std=c++17. Output contains no timestamps: regeneration must be
// byte-identical. This program is the ONLY place QuantLib is used; the Rust
// side consumes fixtures and never mirrors generator logic.
#include <ql/quantlib.hpp>
#include <sstream>

#include <charconv>
#include <cstdio>
#include <fstream>
#include <iostream>
#include <map>
#include <string>
#include <vector>

using namespace QuantLib;

// ---------- JSON emission (minimal, deterministic) ----------

static std::string jnum(double x) {
    char buf[64];
    auto res = std::to_chars(buf, buf + sizeof(buf), x);
    return std::string(buf, res.ptr);
}

static std::string jstr(const std::string& s) {
    std::string out = "\"";
    for (char c : s) {
        switch (c) {
            case '"': out += "\\\""; break;
            case '\\': out += "\\\\"; break;
            default: out += c;
        }
    }
    return out + "\"";
}

static std::string iso(const Date& d) {
    char buf[16];
    std::snprintf(buf, sizeof(buf), "%04d-%02d-%02d", static_cast<int>(d.year()),
                  static_cast<int>(d.month()), static_cast<int>(d.dayOfMonth()));
    return buf;
}

static const char* weekdayName(const Date& d) {
    switch (d.weekday()) {
        case Sunday: return "Sun";
        case Monday: return "Mon";
        case Tuesday: return "Tue";
        case Wednesday: return "Wed";
        case Thursday: return "Thu";
        case Friday: return "Fri";
        case Saturday: return "Sat";
        default: return "??";
    }
}

struct DateCase {
    std::string id;
    std::string op; // advance_days | weekday | end_of_month | is_leap
    std::string date;
    int n;
};

struct DcCase {
    std::string id;
    std::string counter; // contract enum name
    std::string start, end;
    bool has_ref;
    std::string ref_start, ref_end;
};

struct BsCase {
    std::string id;
    std::string opt_type; // call | put
    double spot, strike, rate, div, vol;
    int maturity_days;
};

static DayCounter make_counter(const std::string& name) {
    if (name == "Actual360") return Actual360();
    if (name == "Actual365Fixed") return Actual365Fixed();
    if (name == "Thirty360USA") return Thirty360(Thirty360::USA);
    if (name == "Thirty360European") return Thirty360(Thirty360::European);
    if (name == "ActualActualISDA") return ActualActual(ActualActual::ISDA);
    QL_FAIL("unknown counter: " + name);
}

int main(int argc, char** argv) {
    std::string out_dir = "fixtures";
    for (int i = 1; i + 1 < argc; ++i) {
        if (std::string(argv[i]) == "--out-dir") out_dir = argv[i + 1];
    }

    // ---------- scenario tables (fixed order => deterministic output) ----------
    std::vector<DateCase> date_cases = {
        {"dt_adv_0001", "advance_days", "2024-02-28", 2},
        {"dt_adv_0002", "advance_days", "2023-01-31", -10},
        {"dt_adv_0003", "advance_days", "2023-12-20", 20},
        {"dt_adv_0004", "advance_days", "1901-01-01", 1},
        {"dt_adv_0005", "advance_days", "2199-12-30", 1},
        {"dt_wd_0001", "weekday", "2023-06-21", 0},
        {"dt_wd_0002", "weekday", "2024-02-29", 0},
        {"dt_wd_0003", "weekday", "1901-01-01", 0},
        {"dt_wd_0004", "weekday", "2000-01-01", 0},
        {"dt_eom_0001", "end_of_month", "2023-02-10", 0},
        {"dt_eom_0002", "end_of_month", "2024-02-10", 0},
        {"dt_eom_0003", "end_of_month", "2023-01-01", 0},
        {"dt_eom_0004", "end_of_month", "2023-12-01", 0},
        {"dt_leap_0001", "is_leap", "2024-01-01", 0},
        {"dt_leap_0002", "is_leap", "2023-01-01", 0},
        {"dt_leap_0003", "is_leap", "2000-01-01", 0},
        {"dt_leap_0004", "is_leap", "2100-01-01", 0},
    };

    std::vector<DcCase> dc_cases = {
        // Actual360
        {"dc_a360_0001", "Actual360", "2023-01-15", "2023-07-15", false, "", ""},
        {"dc_a360_0002", "Actual360", "2024-02-28", "2024-03-01", false, "", ""},
        {"dc_a360_0003", "Actual360", "2023-01-01", "2023-12-31", false, "", ""},
        {"dc_a360_0004", "Actual360", "2023-06-21", "2023-06-21", false, "", ""},
        // Actual365Fixed
        {"dc_a365_0001", "Actual365Fixed", "2023-06-21", "2024-06-21", false, "", ""},
        {"dc_a365_0002", "Actual365Fixed", "2024-02-29", "2025-02-28", false, "", ""},
        {"dc_a365_0003", "Actual365Fixed", "2023-01-01", "2023-01-01", false, "", ""},
        {"dc_a365_0004", "Actual365Fixed", "2022-06-30", "2025-06-30", false, "", ""},
        // Thirty360 USA
        {"dc_30u_0001", "Thirty360USA", "2023-01-31", "2023-07-31", false, "", ""},
        {"dc_30u_0002", "Thirty360USA", "2023-01-30", "2023-07-31", false, "", ""},
        {"dc_30u_0003", "Thirty360USA", "2023-02-28", "2023-03-31", false, "", ""},
        {"dc_30u_0004", "Thirty360USA", "2023-01-31", "2023-02-28", false, "", ""},
        {"dc_30u_0005", "Thirty360USA", "2024-02-29", "2024-08-31", false, "", ""},
        // Thirty360 European
        {"dc_30e_0001", "Thirty360European", "2023-01-31", "2023-07-31", false, "", ""},
        {"dc_30e_0002", "Thirty360European", "2023-08-31", "2023-09-30", false, "", ""},
        {"dc_30e_0003", "Thirty360European", "2023-01-31", "2023-03-31", false, "", ""},
        {"dc_30e_0004", "Thirty360European", "2024-02-29", "2024-03-31", false, "", ""},
        // ActualActual ISDA
        {"dc_aisda_0001", "ActualActualISDA", "2023-01-15", "2023-07-15", false, "", ""},
        {"dc_aisda_0002", "ActualActualISDA", "2023-10-15", "2024-04-15", false, "", ""},
        {"dc_aisda_0003", "ActualActualISDA", "2024-01-01", "2024-12-31", false, "", ""},
        {"dc_aisda_0004", "ActualActualISDA", "2023-01-01", "2023-12-31", false, "", ""},
        {"dc_aisda_0005", "ActualActualISDA", "2022-06-30", "2025-06-30", false, "", ""},
        {"dc_aisda_0006", "ActualActualISDA", "2024-02-29", "2025-02-28", false, "", ""},
        {"dc_aisda_0007", "ActualActualISDA", "2024-03-15", "2024-03-15", false, "", ""},
        // ActualActual ISMA (explicit reference period)
        {"dc_aisma_0001", "ActualActualISMA", "2023-06-15", "2023-12-15", true, "2023-06-15", "2023-12-15"},
        {"dc_aisma_0002", "ActualActualISMA", "2023-06-15", "2024-06-15", true, "2023-06-15", "2024-06-15"},
        {"dc_aisma_0003", "ActualActualISMA", "2023-08-15", "2023-12-15", true, "2023-06-15", "2023-12-15"},
        {"dc_aisma_0004", "ActualActualISMA", "2023-06-15", "2023-10-15", true, "2023-06-15", "2023-12-15"},
        {"dc_aisma_0005", "ActualActualISMA", "2023-04-15", "2023-10-15", true, "2023-04-15", "2023-10-15"},
        {"dc_aisma_0006", "ActualActualISMA", "2023-01-15", "2024-01-15", true, "2023-01-15", "2024-01-15"},
    };

    Date today(21, June, 2023);
    std::vector<BsCase> bs_cases = {
        {"bs_call_base", "call", 100.0, 105.0, 0.05, 0.02, 0.20, 365},
        {"bs_put_base", "put", 100.0, 105.0, 0.05, 0.02, 0.20, 365},
        {"bs_call_itm", "call", 120.0, 80.0, 0.03, 0.01, 0.15, 730},
        {"bs_put_otm", "put", 50.0, 120.0, 0.04, 0.00, 0.30, 180},
        {"bs_call_short", "call", 100.0, 100.0, 0.02, 0.02, 0.25, 7},
        {"bs_call_lowvol", "call", 100.0, 90.0, 0.05, 0.02, 0.0001, 365},
        {"bs_call_highvol", "call", 100.0, 100.0, 0.05, 0.02, 0.80, 365},
        {"bs_call_zerorate", "call", 100.0, 100.0, 0.0, 0.0, 0.20, 365},
        {"bs_put_negrates", "put", 100.0, 100.0, -0.005, 0.0, 0.20, 365},
        {"bs_call_long", "call", 100.0, 110.0, 0.05, 0.02, 0.25, 1825},
        // category-driven additions (ADR-0003 §4): ATM, boundary, near-zero
        {"bs_call_atm", "call", 100.0, 100.0, 0.05, 0.02, 0.20, 365},
        {"bs_put_atm", "put", 100.0, 100.0, 0.05, 0.02, 0.20, 365},
        {"bs_call_1d", "call", 100.0, 100.0, 0.03, 0.0, 0.25, 1},
        {"bs_call_nearzero_vol", "call", 100.0, 95.0, 0.05, 0.02, 1e-06, 365},
        {"bs_call_deep_otm", "call", 10.0, 100.0, 0.05, 0.02, 0.30, 365},
        {"bs_put_deep_itm", "put", 200.0, 100.0, 0.05, 0.02, 0.30, 365},
        {"bs_call_10y", "call", 100.0, 100.0, 0.04, 0.02, 0.20, 3652},
        {"bs_put_short_highvol", "put", 100.0, 110.0, 0.02, 0.01, 0.90, 30},
    };

    // ---------- date_daycount family ----------
    std::ofstream f(out_dir + "/date_daycount/cases.json");
    f << "{\n  \"schema\": 1,\n  \"family\": \"date_daycount\",\n  \"cases\": [\n";
    bool first = true;
    for (const auto& c : date_cases) {
        Date d = DateParser::parseISO(c.date);
        std::string expected;
        if (c.op == "advance_days") {
            Date r = d + c.n;
            expected = "{\"iso\": " + jstr(iso(r)) + ", \"weekday\": " + jstr(weekdayName(r)) + "}";
        } else if (c.op == "weekday") {
            expected = "{\"weekday\": " + jstr(weekdayName(d)) + "}";
        } else if (c.op == "end_of_month") {
            Date r = Date::endOfMonth(d);
            expected = "{\"iso\": " + jstr(iso(r)) + ", \"is_last_day_of_feb\": " +
                       (r.month() == Feb ? "true" : "false") + "}";
        } else if (c.op == "is_leap") {
            expected = std::string("{\"is_leap\": ") + (Date::isLeap(d.year()) ? "true" : "false") + "}";
        }
        if (!first) f << ",\n";
        first = false;
        f << "    {\"id\": " << jstr(c.id) << ", \"kind\": \"date\", \"input\": {\"op\": "
          << jstr(c.op) << ", \"date\": " << jstr(c.date) << ", \"days\": " << c.n
          << "}, \"expected\": " << expected << ", \"tolerance\": {\"mode\": \"exact\"}}";
    }
    for (const auto& c : dc_cases) {
        Date d1 = DateParser::parseISO(c.start), d2 = DateParser::parseISO(c.end);
        Real yf;
        if (c.counter == "ActualActualISMA") {
            ActualActual dc(ActualActual::ISMA);
            yf = dc.yearFraction(d1, d2, DateParser::parseISO(c.ref_start), DateParser::parseISO(c.ref_end));
        } else {
            yf = make_counter(c.counter).yearFraction(d1, d2);
        }
        BigInteger days = d2 - d1;
        std::string input = "{\"day_counter\": " + jstr(c.counter) + ", \"start\": " +
                            jstr(c.start) + ", \"end\": " + jstr(c.end);
        if (c.has_ref)
            input += ", \"ref_period_start\": " + jstr(c.ref_start) +
                     ", \"ref_period_end\": " + jstr(c.ref_end);
        input += "}";
        if (!first) f << ",\n";
        first = false;
        f << "    {\"id\": " << jstr(c.id) << ", \"kind\": \"daycount\", \"input\": " << input
          << ", \"expected\": {\"year_fraction\": " << jnum(yf) << ", \"actual_days\": " << days
          << "}, \"tolerance\": {\"mode\": \"mixed\", \"exact_metrics\": [\"actual_days\"], "
             "\"float_metrics\": {\"year_fraction\": {\"atol\": 1e-15, \"rtol\": 1e-12, "
             "\"scale_type\": \"expected_magnitude\"}}}}";
    }
    f << "\n  ]\n}\n";
    f.close();

    // ---------- black_scholes_preview family ----------
    Settings::instance().evaluationDate() = today;
    std::ofstream g(out_dir + "/black_scholes/cases.json");
    g << "{\n  \"schema\": 1,\n  \"family\": \"black_scholes_preview\",\n  \"status\": "
         "\"preview_m1\",\n  \"cases\": [\n";
    first = true;
    Actual365Fixed dc365;
    for (const auto& c : bs_cases) {
        Date maturity = today + c.maturity_days;
        Handle<Quote> spot(ext::make_shared<SimpleQuote>(c.spot));
        Handle<Quote> rateQ(ext::make_shared<SimpleQuote>(c.rate));
        Handle<Quote> divQ(ext::make_shared<SimpleQuote>(c.div));
        Handle<Quote> volQ(ext::make_shared<SimpleQuote>(c.vol));
        Handle<YieldTermStructure> rts(
            ext::make_shared<FlatForward>(today, rateQ, dc365, Continuous, Annual));
        Handle<YieldTermStructure> dts(
            ext::make_shared<FlatForward>(today, divQ, dc365, Continuous, Annual));
        Handle<BlackVolTermStructure> vts(ext::make_shared<BlackConstantVol>(
            today, NullCalendar(), volQ, dc365));
        auto stoch = ext::make_shared<BlackScholesMertonProcess>(spot, dts, rts, vts);
        auto payoff = ext::make_shared<PlainVanillaPayoff>(
            c.opt_type == "call" ? Option::Call : Option::Put, c.strike);
        auto exercise = ext::make_shared<EuropeanExercise>(maturity);
        EuropeanOption option(payoff, exercise);
        option.setPricingEngine(ext::make_shared<AnalyticEuropeanEngine>(stoch));
        Real T = dc365.yearFraction(today, maturity);
        if (!first) g << ",\n";
        first = false;
        g << "    {\"id\": " << jstr(c.id) << ", \"input\": {\"option_type\": "
          << jstr(c.opt_type) << ", \"spot\": " << jnum(c.spot) << ", \"strike\": "
          << jnum(c.strike) << ", \"risk_free\": " << jnum(c.rate)
          << ", \"dividend\": " << jnum(c.div) << ", \"vol\": " << jnum(c.vol)
          << ", \"eval_date\": " << jstr(iso(today)) << ", \"maturity_date\": "
          << jstr(iso(maturity)) << ", \"day_counter\": \"Actual365Fixed\", \"T\": " << jnum(T)
          << "}, \"expected\": {\"npv\": " << jnum(option.NPV())
          << ", \"delta\": " << jnum(option.delta()) << ", \"gamma\": " << jnum(option.gamma())
          << ", \"vega\": " << jnum(option.vega()) << ", \"theta\": " << jnum(option.theta())
          << "}, \"tolerance\": {\"mode\": \"mixed\", \"exact_metrics\": [], "
             "\"float_metrics\": {\"npv\": {\"atol\": 1e-10, \"rtol\": 1e-9, \"scale_type\": "
             "\"unit_price\"}, \"delta\": {\"atol\": 1e-10, \"rtol\": 1e-9, \"scale_type\": "
             "\"expected_magnitude\"}, \"gamma\": {\"atol\": 1e-12, \"rtol\": 1e-9, "
             "\"scale_type\": \"expected_magnitude\"}, \"vega\": {\"atol\": 1e-10, \"rtol\": "
             "1e-9, \"scale_type\": \"expected_magnitude\"}, \"theta\": {\"atol\": 1e-9, "
             "\"rtol\": 1e-9, \"scale_type\": \"expected_magnitude\"}}}}";
    }
    g << "\n  ]\n}\n";
    g.close();

    // ---------- calendar family (M2) ----------
    // The date semantics below (spot, schedule dates, pillars) ARE the
    // contract: the Rust side mirrors what is dumped here, not its memory
    // of QuantLib behavior.
    std::ofstream h(out_dir + "/calendar/cases.json");
    h << "{\n  \"schema\": 1,\n  \"family\": \"calendar\",\n  \"cases\": [\n";
    first = true;
    {
        Calendar target = TARGET(), weekends = WeekendsOnly(), nul = NullCalendar();
        auto emit = [&](const std::string& id, const std::string& cal,
                        const std::string& op, const std::string& date_in,
                        const std::string& extra_key, const std::string& extra_val,
                        const std::string& expected_json) {
            if (!first) h << ",\n";
            first = false;
            h << "    {\"id\": " << jstr(id) << ", \"kind\": \"calendar\", \"input\": {\"calendar\": "
              << jstr(cal) << ", \"op\": " << jstr(op) << ", \"date\": " << jstr(date_in);
            if (!extra_key.empty())
                h << ", " << jstr(extra_key) << ": " << extra_val;
            h << "}, \"expected\": " << expected_json << ", \"tolerance\": {\"mode\": \"exact\"}}";
        };
        // business-day grid 2023-2026 + Easter edges (Good Friday/Easter Monday)
        std::vector<std::string> grid = {
            "2023-01-02", "2023-04-07", "2023-04-10", "2023-05-01", "2023-12-25", "2023-12-26",
            "2024-01-01", "2024-03-29", "2024-04-01", "2024-12-24", "2024-12-31",
            "2025-04-18", "2025-04-21", "2025-05-01", "2026-04-03", "2026-04-06",
            "2023-06-21", "2024-06-21", "2025-06-21", "2026-06-21",
            "2024-02-29", "2025-12-26", "2026-01-01"};
        int n = 0;
        for (auto& s : grid) {
            Date d = DateParser::parseISO(s);
            emit("cal_bday_" + std::to_string(++n), "TARGET", "is_business_day", s, "",
                 "", std::string("{\"is_business_day\": ") + (target.isBusinessDay(d) ? "true" : "false") + "}");
            emit("cal_wk_bday_" + std::to_string(n), "WeekendsOnly", "is_business_day", s, "",
                 "", std::string("{\"is_business_day\": ") + (weekends.isBusinessDay(d) ? "true" : "false") + "}");
        }
        // adjust with four conventions
        std::vector<std::pair<std::string, BusinessDayConvention>> convs = {
            {"Following", Following}, {"ModifiedFollowing", ModifiedFollowing},
            {"Preceding", Preceding}, {"Unadjusted", Unadjusted}};
        std::vector<std::string> adj_dates = {"2023-01-01", "2023-04-07", "2024-06-15",
                                              "2024-12-25", "2025-05-01", "2026-12-26", "2023-06-21"};
        n = 0;
        for (auto& s : adj_dates) {
            Date d = DateParser::parseISO(s);
            for (auto& [cname, conv] : convs) {
                Date r = target.adjust(d, conv);
                emit("cal_adj_" + std::to_string(++n), "TARGET", "adjust:" + cname, s,
                     "", "", "{\"iso\": " + jstr(iso(r)) + ", \"weekday\": " + jstr(weekdayName(r)) + "}");
            }
        }
        // advance by months with conventions (schedule-building primitive)
        n = 0;
        std::vector<std::tuple<std::string, int, std::string>> advs = {
            {"2023-06-21", 6, "Following"}, {"2023-06-30", 6, "ModifiedFollowing"},
            {"2024-02-29", 6, "ModifiedFollowing"}, {"2023-08-31", 12, "ModifiedFollowing"},
            {"2023-01-31", 1, "ModifiedFollowing"}, {"2023-10-31", 3, "Following"}};
        for (auto& [s, months, cname] : advs) {
            Date d = DateParser::parseISO(s);
            BusinessDayConvention conv = cname == "Following" ? Following : ModifiedFollowing;
            Date r = target.advance(d, months, Months, conv);
            emit("cal_adv_" + std::to_string(++n), "TARGET", "advance_months:" + cname, s,
                 "months", std::to_string(months), "{\"iso\": " + jstr(iso(r)) + ", \"weekday\": " + jstr(weekdayName(r)) + "}");
        }
    }
    h << "\n  ]\n}\n";
    h.close();

    // ---------- rates family (M2): curve bootstrap + swap pricing ----------
    std::ofstream k(out_dir + "/rates/cases.json");
    k << "{\n  \"schema\": 1,\n  \"family\": \"rates\",\n  \"cases\": [\n";
    first = true;
    {
        Date today(21, June, 2023);
        Settings::instance().evaluationDate() = today;
        Calendar cal = TARGET();
        Date spot = cal.advance(today, 2, Days);
        RelinkableHandle<YieldTermStructure> curveHandle;
        auto index = ext::make_shared<IborIndex>(
            "Euribor6M", 6 * Months, 2, EURCurrency(), cal, ModifiedFollowing,
            false, Actual360(), curveHandle);

        struct Scenario { std::string id; Real dep6m; std::vector<Real> swaps; };
        std::vector<Scenario> scenarios = {
            {"flat", 0.030, {0.030, 0.030, 0.030, 0.030, 0.030, 0.030, 0.030, 0.030, 0.030, 0.030}},
            {"upward", 0.025, {0.027, 0.028, 0.029, 0.030, 0.031, 0.032, 0.033, 0.034, 0.035, 0.036}},
            {"inverted", 0.040, {0.038, 0.036, 0.034, 0.033, 0.032, 0.031, 0.030, 0.029, 0.028, 0.027}},
        };

        Actual365Fixed dc365;
        Thirty360 dc30e(Thirty360::European);

        for (auto& sc : scenarios) {
            std::vector<ext::shared_ptr<RateHelper>> helpers;
            helpers.push_back(ext::make_shared<DepositRateHelper>(
                sc.dep6m, 6 * Months, 2, cal, ModifiedFollowing, false, Actual360()));
            for (int i = 0; i < 10; ++i)
                helpers.push_back(ext::make_shared<SwapRateHelper>(
                    sc.swaps[i], (i + 1) * Years, cal, Annual, ModifiedFollowing,
                    dc30e, index));

            auto curve = ext::make_shared<PiecewiseYieldCurve<Discount, LogLinear>>(
                spot, helpers, dc365);
            curve->discount(1.0); // force bootstrap

            // dump pillars + discounts + zero rates + helper residuals
            std::ostringstream nodes, zeros, residuals;
            for (Size i = 0; i < curve->dates().size(); ++i) {
                if (i) { nodes << ","; zeros << ","; }
                nodes << "\"" << iso(curve->dates()[i]) << "\":" << jnum(curve->data()[i]);
                zeros << "\"" << iso(curve->dates()[i]) << "\":" << jnum(curve->zeroRate(curve->dates()[i], dc365, Continuous).rate());
            }
            for (Size i = 0; i < helpers.size(); ++i) {
                if (i) residuals << ",";
                residuals << jnum(helpers[i]->impliedQuote() - helpers[i]->quote()->value());
            }

            curveHandle.linkTo(curve);

            // swap cases per scenario: payer/receiver 5Y off-market, payer 10Y
            struct SwapCase { std::string id; VanillaSwap::Type type; int years; Real fixed_rate; };
            std::vector<SwapCase> swaps = {
                {sc.id + "_payer5y_off", VanillaSwap::Type::Payer, 5, 0.020},
                {sc.id + "_rec10y_off", VanillaSwap::Type::Receiver, 10, 0.045},
                {sc.id + "_payer3y_atm", VanillaSwap::Type::Payer, 3, 0.0},
            };
            for (auto& swp : swaps) {
                Real fixedRate = swp.fixed_rate == 0.0 ? 0.0 : swp.fixed_rate;
                Date maturity = cal.advance(spot, swp.years, Years, Unadjusted);
                Schedule fs(spot, maturity, Period(Annual), cal, ModifiedFollowing,
                            ModifiedFollowing, DateGeneration::Forward, false);
                Schedule vs(spot, maturity, Period(Semiannual), cal, ModifiedFollowing,
                            ModifiedFollowing, DateGeneration::Forward, false);
                VanillaSwap swap(swp.type, 1.0, fs, fixedRate, dc30e, vs, index, 0.0, Actual360());
                swap.setPricingEngine(ext::make_shared<DiscountingSwapEngine>(curveHandle));
                Real fair = swap.fairRate();
                if (swp.fixed_rate == 0.0) { // atm case: re-price at fair
                    fixedRate = fair; // the dumped input must carry the fair rate
                    swap = VanillaSwap(swp.type, 1.0, fs, fair, dc30e, vs, index, 0.0, Actual360());
                    swap.setPricingEngine(ext::make_shared<DiscountingSwapEngine>(curveHandle));
                }
                // dump legs: dates + year fractions + amounts (fixed) / dates (float)
                std::ostringstream fixleg, floatleg;
                const auto& fleg = swap.fixedLeg();
                for (Size i = 0; i < fleg.size(); ++i) {
                    auto cpn = ext::dynamic_pointer_cast<FixedRateCoupon>(fleg[i]);
                    if (i) fixleg << ",";
                    fixleg << "{\"date\": \"" << iso(cpn->date()) << "\", \"yf\": "
                           << jnum(dc30e.yearFraction(cpn->accrualStartDate(), cpn->accrualEndDate()))
                           << ", \"amount\": " << jnum(cpn->amount()) << "}";
                }
                const auto& lleg = swap.floatingLeg();
                for (Size i = 0; i < lleg.size(); ++i) {
                    auto cpn = ext::dynamic_pointer_cast<FloatingRateCoupon>(lleg[i]);
                    if (i) floatleg << ",";
                    floatleg << "{\"date\": \"" << iso(cpn->date()) << "\", \"yf\": "
                           << jnum(cpn->accrualPeriod()) << "}";
                }
                if (!first) k << ",\n";
                first = false;
                k << "    {\"id\": " << jstr(swp.id) << ", \"kind\": \"swap\", \"input\": {\"scenario\": "
                  << jstr(sc.id) << ", \"eval_date\": " << jstr(iso(today))
                  << ", \"swap_type\": " << jstr(swp.type == VanillaSwap::Type::Payer ? "payer" : "receiver")
                  << ", \"years\": " << swp.years << ", \"fixed_rate\": " << jnum(fixedRate)
                  << "}, \"expected\": {\"npv\": " << jnum(swap.NPV())
                  << ", \"fair_rate\": " << jnum(fair)
                  << ", \"fixed_leg_npv\": " << jnum(swap.fixedLegNPV())
                  << ", \"floating_leg_npv\": " << jnum(swap.floatingLegNPV())
                  << ", \"fixed_leg\": [" << fixleg.str() << "]"
                  << ", \"float_leg_dates\": [" << floatleg.str() << "]"
                  << "}, \"tolerance\": {\"mode\": \"mixed\", \"exact_metrics\": [], "
                     "\"float_metrics\": {\"npv\": {\"atol\": 1e-10, \"rtol\": 1e-9, \"scale_type\": "
                     "\"unit_notional\"}, \"fair_rate\": {\"atol\": 1e-11, \"rtol\": 1e-9, "
                     "\"scale_type\": \"expected_magnitude\"}, \"fixed_leg_npv\": {\"atol\": 1e-10, "
                     "\"rtol\": 1e-9, \"scale_type\": \"unit_notional\"}, \"floating_leg_npv\": "
                     "{\"atol\": 1e-10, \"rtol\": 1e-9, \"scale_type\": \"unit_notional\"}}}}";
            }
            if (!first) k << ",\n";
            first = false;
            k << "    {\"id\": " << jstr("curve_" + sc.id) << ", \"kind\": \"curve\", \"input\": {\"eval_date\": "
              << jstr(iso(today)) << ", \"deposit_6m\": " << jnum(sc.dep6m) << ", \"swap_rates_1y_10y\": [";
            for (int i = 0; i < 10; ++i) k << (i ? "," : "") << jnum(sc.swaps[i]);
            k << "]}, \"expected\": {\"spot_date\": " << jstr(iso(spot))
              << ", \"pillar_dates\": {" << nodes.str() << "}"
              << ", \"zero_rates_cont_365\": {" << zeros.str() << "}"
              << ", \"helper_residuals\": [" << residuals.str() << "]"
              << "}, \"tolerance\": {\"mode\": \"mixed\", \"exact_metrics\": [], \"float_metrics\": "
                 "{\"df_max_abs\": {\"atol\": 1e-10, \"rtol\": 1e-9, \"scale_type\": \"expected_magnitude\"}}}}";
        }
    }
    k << "\n  ]\n}\n";
    k.close();

    std::cout << "fixtures written to " << out_dir << "\n";
    return 0;
}
