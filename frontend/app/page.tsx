export default function Home() {
  return (
    <main className="welcome">
      <div className="mark" aria-hidden="true">T</div>
      <p className="eyebrow">Your workspace starts here</p>
      <h1>Make work flow.</h1>
      <p className="description">
        Treefime gives your team a clear place to plan projects, share progress, and get things done.
      </p>
      <div className="preview" aria-label="A preview of a project board">
        <section className="column">
          <div className="column-title"><span className="dot violet" />To do<span className="count">2</span></div>
          <article className="card"><span className="tag lilac">Planning</span><strong>Map out the next big idea</strong><small>☷ &nbsp;3 tasks</small></article>
          <article className="card"><span className="tag blue">Design</span><strong>Gather inspiration</strong><small>◷ &nbsp;Tomorrow</small></article>
        </section>
        <section className="column">
          <div className="column-title"><span className="dot amber" />In progress<span className="count">1</span></div>
          <article className="card"><span className="tag green">Product</span><strong>Build something wonderful</strong><small>☷ &nbsp;2 tasks &nbsp; · &nbsp; ◉ 2</small></article>
        </section>
        <section className="column">
          <div className="column-title"><span className="dot mint" />Done<span className="count">1</span></div>
          <article className="card"><span className="tag rose">Team</span><strong>Bring everyone together</strong><small>✓ &nbsp;Completed</small></article>
        </section>
      </div>
      <p className="footer-note">A calmer way to move forward.</p>
    </main>
  );
}
