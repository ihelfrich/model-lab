//! Original lessons and reasoning checks, independent of presentation widgets.
use crate::linear::LinearMethod;
pub struct LessonSection {
    pub title: &'static str,
    pub body: &'static str,
    pub experiment: &'static str,
}
pub struct Question {
    pub prompt: &'static str,
    pub options: [&'static str; 4],
    pub answer: usize,
    pub explanation: &'static str,
}
pub struct Lesson {
    pub title: &'static str,
    pub objective: &'static str,
    pub equation: &'static str,
    pub sections: Vec<LessonSection>,
    pub questions: Vec<Question>,
}
pub fn lesson(method: LinearMethod) -> Lesson {
    match method{
        LinearMethod::Ols=>Lesson{title:"OLS: projection and residuals",objective:"Connect a least-squares fit to its residual geometry, coefficient interpretation and specification limits.",equation:"minimize  RSS / (2*n), where RSS = sum_i (y_i - b0 - sum_j z_ij*b_j)^2",sections:vec![
            LessonSection{title:"The fitted value is a projection",body:"OLS chooses the vector of predictions closest to the training outcomes in squared Euclidean distance, within the span of the included predictor columns and intercept. The native solver builds an orthogonal basis. Each recorded QR stage adds an independent direction and computes the actual nested projection.",experiment:"Move from the baseline to the final projection. Compare how the residuals change when another independent direction is added."},
            LessonSection{title:"Read a coefficient in its stated units",body:"The coefficient table reports original-unit slopes by default. A slope is the change in the linear prediction for a one-unit increase in that predictor while the other included predictors stay fixed. Standardized coefficients instead describe a training-standard-deviation change. Neither coefficient scale establishes causation.",experiment:"Select an observation. Reconstruct its fitted value by adding the intercept and every coefficient × predictor contribution."},
            LessonSection{title:"Correlated predictors and nonuniqueness",body:"Highly correlated predictors can give similar predictions with unstable individual coefficients. Exactly dependent columns make the coefficient vector nonunique. The solver reports its numerical rank and returns a basic least-squares solution; this is not a minimum-norm guarantee.",experiment:"Choose Correlated predictors. Compare OLS with ridge using the same observations and split."},
            LessonSection{title:"A good optimizer cannot repair a bad specification",body:"A small stationarity residual checks the numerical optimum. It does not establish that a linear conditional mean is appropriate, that an observed relationship is causal, or that validation performance will transfer to a different population.",experiment:"Choose Nonlinear signal. Inspect the fitted slice and residual pattern, then compare with a tree-based fit."},
        ],questions:vec![
            Question{prompt:"With an intercept, what should full-rank OLS training residuals be orthogonal to?",options:["Only the fitted intercept","Every included predictor column and the intercept","Every variable in the source file","The validation outcomes"],answer:1,explanation:"The training normal equations set Xᵀe to zero. This concerns the included design and the training sample, not arbitrary variables or validation rows."},
            Question{prompt:"Two predictors become nearly identical. Which outcome is plausible?",options:["Stable predictions but unstable individual slopes","Guaranteed lower validation error","A unique causal interpretation","No numerical or statistical consequences"],answer:0,explanation:"Near-collinearity makes the allocation between related coefficients unstable even when their combined prediction changes little."},
            Question{prompt:"A training-only standardization changes predictor units. What happens to an unpenalized full-rank OLS fit?",options:["The fitted predictions must change","Coefficients change units; fitted predictions are preserved up to numerical error","The response becomes standardized automatically","The intercept must become zero"],answer:1,explanation:"An invertible rescaling preserves the design span. The reported coefficients must be transformed back to the corresponding units."},
            Question{prompt:"A small numerical stationarity residual establishes which claim?",options:["Causal identification","Correct population specification","The implemented optimum satisfies its numerical first-order check","Reliable forecasting outside the data range"],answer:2,explanation:"A numerical check concerns the fitted objective under the chosen model. Identification, specification and extrapolation require different evidence."},
        ]},
        LinearMethod::Logistic=>Lesson{title:"Logistic regression: scores, probabilities and decisions",objective:"Trace a linear score through the logistic link, likelihood optimization and a chosen classification threshold.",equation:"eta_i = b0 + sum_j z_ij*b_j;  p_i = 1 / (1 + exp(-eta_i))",sections:vec![
            LessonSection{title:"The linear predictor is a log-odds score",body:"A coefficient adds to log odds, not directly to probability. Applying the sigmoid converts the score to a probability. The same score change has different probability effects near the middle and near the extremes of the sigmoid.",experiment:"Select observations with probabilities near 0.5 and near 0 or 1. Compare their scores and predicted probabilities."},
            LessonSection{title:"Likelihood uses the probability of the observed label",body:"A positive observation contributes −log(p); a negative observation contributes −log(1−p). The engine minimizes their training average. Newton updates use the score and curvature; backtracking accepts an update only when the actual objective decreases.",experiment:"Step through Optimization. Compare the recorded likelihood objective before and after each Newton update."},
            LessonSection{title:"A probability is not yet a class decision",body:"The threshold determines how a probability becomes a predicted class. Changing it changes the confusion matrix, sensitivity and specificity, while leaving the fitted coefficients, predicted probabilities and probability-based log loss unchanged.",experiment:"Change the decision threshold from 0.3 to 0.7. Explain which observations change class and why."},
            LessonSection{title:"Separation is an estimation failure to recognize",body:"If a linear score strictly separates all positive and negative training observations, increasing its magnitude can improve the unpenalized likelihood without reaching a finite maximum. The engine reports a discovered separating direction and does not label it a converged finite MLE.",experiment:"Compare overlap and separation conceptually. A low classification error is not by itself evidence that a finite unpenalized MLE exists."},
        ],questions:vec![
            Question{prompt:"A log-odds score is zero. What probability does the sigmoid produce?",options:["0","0.25","0.5","1"],answer:2,explanation:"sigmoid(0)=1/(1+1)=0.5."},
            Question{prompt:"A negative observation receives probability 0.9 of class 1. Its likelihood contribution is:",options:["0.9","0.1","−0.9","The classification threshold"],answer:1,explanation:"The observed label is 0, so its probability is 1−p=0.1. Its negative log-likelihood contribution is −log(0.1)."},
            Question{prompt:"Only the decision threshold changes. Which quantity remains unchanged?",options:["Every predicted class","The confusion matrix","The fitted probabilities and log loss","The count of predicted positives"],answer:2,explanation:"The threshold acts after probability estimation. It changes decisions, not the probability model."},
            Question{prompt:"A coefficient of log(2) means a one-unit predictor increase, holding others fixed:",options:["Adds 2 to probability","Doubles the odds","Doubles probability everywhere","Changes accuracy by 2%"],answer:1,explanation:"Exponentiating a log-odds coefficient gives the multiplicative odds ratio. Probability changes depend on the starting probability."},
        ]},
        method=>regularization(method),
    }
}
fn regularization(method: LinearMethod) -> Lesson {
    let (title,equation,mechanism)=match method{
        LinearMethod::Ridge=>("Ridge: continuous shrinkage","minimize  RSS / (2*n) + (alpha/2) * sum_j b_j^2","Ridge adds a quadratic cost to coefficient magnitude. Each coordinate update divides its partial-residual association by a predictor second moment plus α. The penalty generally shrinks coefficients continuously rather than forcing exact zeros."),
        LinearMethod::Lasso=>("Lasso: shrinkage and selection","minimize  RSS / (2*n) + alpha * sum_j abs(b_j)","Lasso applies soft thresholding to a predictor's partial-residual association. Associations inside the threshold produce an exact zero coefficient. A selected variable is useful under this fitted objective; selection does not establish a causal relationship or selection stability."),
        _=>("Elastic net: sparse and stable coefficients","minimize RSS/(2*n) + alpha*mix*sum_j abs(b_j) + alpha*(1-mix)*sum_j b_j^2/2","Elastic net combines lasso's soft threshold with ridge's quadratic shrinkage. In this application, mix=1 is lasso and mix=0 is ridge. α controls total penalty strength. An intermediate mixture can retain correlated predictors while still producing sparse solutions."),
    };
    Lesson{title,objective:"Understand the penalty convention, coordinate updates, coefficient paths and validation tradeoffs.",equation,sections:vec![
        LessonSection{title:"The penalty changes the optimization problem",body:mechanism,experiment:"Increase α. Follow each coefficient and the fitted response instead of assuming every coefficient must move monotonically on a correlated design."},
        LessonSection{title:"Standardization makes the convention visible",body:"The penalty applies to coefficients in the displayed solver scale. With standardization enabled, a coefficient multiplies a predictor measured in training standard deviations. Predictor means and scales come from training rows only. The intercept is never penalized. Original-unit coefficients are recovered for interpretation.",experiment:"Turn standardization off and inspect the coefficient table. Explain why a fixed penalty can behave differently after a change of predictor units."},
        LessonSection{title:"Coordinate descent uses partial residuals",body:"One sweep updates each coefficient in turn, accounting for the current values of the others. Ridge uses a quadratic shrinkage denominator; lasso adds soft thresholding; elastic net uses both. The trace records completed sweeps and their actual objective values. KKT residuals determine convergence.",experiment:"Move between early solver states. Locate a coefficient that becomes zero and compare its role in the selected observation's prediction."},
        LessonSection{title:"Use validation to choose the tradeoff",body:"The coefficient path refits the model at a grid of penalties on the same training partition. Its validation curve helps select penalty strength. Repeated tuning consumes that validation information; it is not an independent final test. A curve point that failed its convergence check is not a reliable candidate.",experiment:"Trace a penalty path, inspect its validation minimum, then compare that fit with a weak and a strong penalty using the same observations."},
    ],questions:vec![
        Question{prompt:"Which quantity is unpenalized in this implementation?",options:["Every predictor coefficient","The intercept","Only the largest coefficient","The validation error"],answer:1,explanation:"The intercept is unpenalized. The penalty applies to predictor coefficients in the chosen solver scale."},
        Question{prompt:"Elastic net with mix=1 is equivalent to:",options:["OLS for all α","Ridge","Lasso","Logistic regression"],answer:2,explanation:"The L2 term disappears and the L1 term has weight α. With mix=0, the reverse endpoint is ridge."},
        Question{prompt:"A lasso coefficient becomes exactly zero when:",options:["Its partial-residual association falls inside the soft threshold","The predictor is causally irrelevant","Its observed values are all negative","The sample contains a validation partition"],answer:0,explanation:"The coordinate update soft-thresholds the partial-residual association. This is an optimization condition, not a causal diagnosis."},
        Question{prompt:"Which rule is appropriate when choosing α?",options:["Minimize the unpenalized training error","Always choose the sparsest model","Use suitable held-out or cross-validation performance, then retain an untouched test set","Choose the largest coefficient norm"],answer:2,explanation:"Penalty choice balances fit and regularization for the task. The training criterion alone does not estimate generalization performance."},
    ]}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn each_new_method_has_complete_original_teaching() {
        for m in LinearMethod::ALL {
            let l = lesson(m);
            assert!(l.sections.len() >= 4);
            assert!(l.questions.len() >= 4);
            for q in l.questions {
                assert!(q.answer < 4);
                assert!(!q.explanation.is_empty());
                assert!(q.options.iter().all(|s| !s.is_empty()));
            }
        }
    }
}
